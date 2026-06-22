//! OpenAI-compatible chat-completions provider.
//!
//! Covers OpenAI itself plus any endpoint that speaks the
//! OpenAI Chat Completions API with SSE:
//! - DeepSeek
//! - Moonshot (Kimi)
//! - Ollama (when run with `--api openai`)
//! - LM Studio
//! - User-defined custom relays
//!
//! Wire format reference: <https://platform.openai.com/docs/api-reference/chat/streaming>

use async_trait::async_trait;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio_util::sync::CancellationToken;

use super::{ChatStream, Provider, ProviderError, StreamChunk};
use crate::message::{Message, Role, ToolCall};

/// An OpenAI-compatible provider.
#[derive(Debug, Clone)]
pub struct OpenAiProvider {
    /// `openai` for OpenAI itself; `openai_compat` for any other
    /// OpenAI-shaped endpoint (DeepSeek, Moonshot, custom relay,
    /// ...). Drives the provider id we surface to the UI.
    pub kind: &'static str,
    /// Base URL (no trailing slash), e.g. `https://api.openai.com/v1`.
    pub base_url: String,
    /// Model id, e.g. `gpt-4o-mini`, `deepseek-chat`.
    pub model: String,
    /// Bearer token.
    pub api_key: String,
}

impl OpenAiProvider {
    /// Build a provider for the OpenAI public API.
    pub fn openai(model: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            kind: "openai",
            base_url: "https://api.openai.com/v1".into(),
            model: model.into(),
            api_key: api_key.into(),
        }
    }

    /// Build a provider for any OpenAI-compatible endpoint.
    pub fn compat(
        base_url: impl Into<String>,
        model: impl Into<String>,
        api_key: impl Into<String>,
    ) -> Self {
        Self {
            kind: "openai_compat",
            base_url: base_url.into(),
            model: model.into(),
            api_key: api_key.into(),
        }
    }
}

#[async_trait]
impl Provider for OpenAiProvider {
    fn id(&self) -> &'static str {
        self.kind
    }

    fn model_id(&self) -> &str {
        &self.model
    }

    async fn stream_chat(
        &self,
        messages: &[Message],
        tools: &[serde_json::Value],
        cancel: CancellationToken,
    ) -> Result<ChatStream, ProviderError> {
        // ── Build request body ────────────────────────────────────
        let body = build_request(messages, tools, &self.model, /*stream*/ true);

        // ── Send ──────────────────────────────────────────────────
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let resp = reqwest::Client::new()
            .post(&url)
            .bearer_auth(&self.api_key)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Api {
                status: status.as_u16(),
                body: body.chars().take(500).collect(),
            });
        }

        // ── SSE stream ────────────────────────────────────────────
        let byte_stream = resp.bytes_stream();
        let sse = eventsource_stream::EventStream::new(byte_stream);
        let model = self.model.clone();

        let stream = async_stream::stream! {
            // We accumulate partial tool-call deltas across SSE
            // events because OpenAI streams one tool argument at
            // a time (by index).
            let mut partial_calls: HashMap<u32, PartialToolCall> = HashMap::new();
            let mut finish_reason: Option<String> = None;

            tokio::pin!(sse);
            loop {
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => break,
                    next = sse.next() => {
                        let Some(event) = next else { break };
                        let event = match event {
                            Ok(e) => e,
                            Err(e) => {
                                yield Err(ProviderError::MalformedStream(e.to_string()));
                                break;
                            }
                        };

                        // SSE spec: when no `event:` field is given, the
                        // event type is `message`. OpenAI's streaming
                        // endpoint doesn't emit `event:` at all, so
                        // eventsource-stream surfaces it as "message".
                        // Treat both "message" and "data" as chat-chunk
                        // carriers.
                        let et = event.event.as_str();
                        if et != "data" && et != "message" {
                            tracing::debug!(event = %et, "ignoring SSE event");
                            continue;
                        }
                        let data = event.data;
                        tracing::debug!(data = %data.chars().take(120).collect::<String>(), "SSE data");
                        if data.trim() == "[DONE]" {
                            yield Ok(StreamChunk::Done {
                                reason: finish_reason.clone().unwrap_or_else(|| "stop".into()),
                            });
                            break;
                        }
                        let chunk: ChatCompletionChunk = match serde_json::from_str(&data) {
                            Ok(c) => c,
                            Err(e) => {
                                yield Err(ProviderError::MalformedStream(format!(
                                    "{} in chunk: {}", e, data.chars().take(200).collect::<String>()
                                )));
                                continue;
                            }
                        };

                        if let Some(choice) = chunk.choices.into_iter().next() {
                            if let Some(r) = choice.finish_reason {
                                finish_reason = Some(r);
                            }
                            if let Some(content) = choice.delta.content {
                                if !content.is_empty() {
                                    yield Ok(StreamChunk::Text { delta: content });
                                }
                            }
                            for tc in choice.delta.tool_calls.unwrap_or_default() {
                                let entry = partial_calls.entry(tc.index).or_insert_with(|| {
                                    let id = tc.id.clone().unwrap_or_default();
                                    let name = tc.function.as_ref().and_then(|f| f.name.clone()).unwrap_or_default();
                                    PartialToolCall { id, name, args: String::new() }
                                });
                                if let Some(id) = tc.id { entry.id = id; }
                                if let Some(func) = tc.function {
                                    if let Some(n) = func.name { entry.name = n; }
                                    if let Some(a) = func.arguments {
                                        entry.args.push_str(&a);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Flush any accumulated tool calls.
            // Sort by index for deterministic ordering.
            let mut indices: Vec<u32> = partial_calls.keys().copied().collect();
            indices.sort();
            for idx in indices {
                if let Some(p) = partial_calls.remove(&idx) {
                    let args: serde_json::Value = if p.args.is_empty() {
                        serde_json::json!({})
                    } else {
                        serde_json::from_str(&p.args).unwrap_or(serde_json::json!({}))
                    };
                    yield Ok(StreamChunk::ToolUse {
                        call: ToolCall {
                            id: if p.id.is_empty() {
                                format!("call_{}_{idx}", model)
                            } else {
                                p.id
                            },
                            name: p.name,
                            args,
                        },
                    });
                }
            }
        };

        Ok(Box::pin(stream))
    }
}

// ── Wire types ────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<RequestMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<serde_json::Value>,
    stream: bool,
}

#[derive(Debug, Serialize)]
#[serde(tag = "role", rename_all = "lowercase")]
enum RequestMessage {
    System { content: String },
    User { content: String },
    Assistant {
        #[serde(skip_serializing_if = "String::is_empty")]
        content: String,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        tool_calls: Vec<RequestToolCall>,
    },
    Tool {
        tool_call_id: String,
        content: String,
    },
}

#[derive(Debug, Serialize)]
struct RequestToolCall {
    id: String,
    #[serde(rename = "type")]
    kind: &'static str, // always "function"
    function: RequestFunction,
}

#[derive(Debug, Serialize)]
struct RequestFunction {
    name: String,
    arguments: String, // JSON-encoded
}

fn build_request<'a>(
    messages: &'a [Message],
    tools: &'a [serde_json::Value],
    model: &'a str,
    stream: bool,
) -> ChatRequest<'a> {
    let mut out_msgs = Vec::with_capacity(messages.len());
    for m in messages {
        match m.role {
            Role::System => out_msgs.push(RequestMessage::System { content: m.content.clone() }),
            Role::User => out_msgs.push(RequestMessage::User { content: m.content.clone() }),
            Role::Assistant => {
                out_msgs.push(RequestMessage::Assistant {
                    content: m.content.clone(),
                    tool_calls: m.tool_calls.iter().map(|tc| RequestToolCall {
                        id: tc.id.clone(),
                        kind: "function",
                        function: RequestFunction {
                            name: tc.name.clone(),
                            arguments: tc.args.to_string(),
                        },
                    }).collect(),
                });
            }
            Role::Tool => {
                out_msgs.push(RequestMessage::Tool {
                    tool_call_id: m.tool_call_id.clone().unwrap_or_default(),
                    content: m.content.clone(),
                });
            }
        }
    }
    ChatRequest {
        model,
        messages: out_msgs,
        tools: tools.to_vec(),
        stream,
    }
}

#[derive(Debug, Deserialize)]
struct ChatCompletionChunk {
    #[serde(default)]
    choices: Vec<ChunkChoice>,
}

#[derive(Debug, Deserialize)]
struct ChunkChoice {
    delta: ChunkDelta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ChunkDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ChunkToolCall>>,
}

#[derive(Debug, Deserialize)]
struct ChunkToolCall {
    index: u32,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<ChunkFunction>,
}

#[derive(Debug, Default, Deserialize)]
struct ChunkFunction {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Debug, Default)]
struct PartialToolCall {
    id: String,
    name: String,
    args: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_serializes_with_all_roles() {
        let mut msgs = vec![
            Message::system("sys"),
            Message::user("hi"),
        ];
        msgs.push(Message::assistant(
            "ok",
            vec![ToolCall {
                id: "c1".into(),
                name: "bash".into(),
                args: serde_json::json!({"cmd": "ls"}),
            }],
        ));
        msgs.push(Message::tool("c1", "out"));
        let body = build_request(&msgs, &[], "m", true);
        let json = serde_json::to_string(&body).unwrap();
        assert!(json.contains("\"role\":\"system\""));
        assert!(json.contains("\"role\":\"tool\""));
        assert!(json.contains("\"tool_calls\""));
    }
}