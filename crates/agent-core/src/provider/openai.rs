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

/// Validate a base URL passed in by the user (ChatZone input,
/// env var, …). Catches the common mistakes before we waste a
/// network round trip on a 30-second TLS handshake to nowhere.
///
/// Returns the cleaned-up form (trailing slash stripped).
pub fn validate_base_url(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("base URL is empty".into());
    }
    let parsed =
        url::Url::parse(trimmed).map_err(|e| format!("base URL is not a valid URL ({e})"))?;
    match parsed.scheme() {
        "http" | "https" => {}
        other => {
            return Err(format!(
                "base URL scheme must be http or https (got {other})"
            ))
        }
    }
    if parsed.host_str().is_none_or(str::is_empty) {
        return Err("base URL is missing a host".into());
    }
    let mut s = parsed.to_string();
    // Normalise: strip trailing slash for consistency with
    // `OpenAiProvider::compat` (which does `.trim_end_matches('/')`).
    if s.ends_with('/') {
        s.pop();
    }
    Ok(s)
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

    /// Build a provider for any OpenAI-compatible endpoint after
    /// validating the URL. Returns a clean error if the URL is
    /// malformed, instead of letting the HTTP layer surface a
    /// confusing `connection refused` / `invalid URL` later.
    pub fn compat_checked(
        base_url: &str,
        model: impl Into<String>,
        api_key: impl Into<String>,
    ) -> Result<Self, String> {
        let cleaned = validate_base_url(base_url)?;
        Ok(Self::compat(cleaned, model, api_key))
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
                                    PartialToolCall {
                                        id: String::new(),
                                        name: String::new(),
                                        args: String::new(),
                                    }
                                });
                                if let Some(id) = tc.id {
                                    if entry.id.is_empty() {
                                        entry.id = id;
                                    } else {
                                        entry.id.push_str(&id);
                                    }
                                }
                                if let Some(func) = tc.function {
                                    if let Some(n) = func.name {
                                        entry.name.push_str(&n);
                                    }
                                    if let Some(a) = func.arguments {
                                        entry.args.push_str(&a);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Flush any accumulated tool calls BEFORE yielding Done.
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

            yield Ok(StreamChunk::Done {
                reason: finish_reason.unwrap_or_else(|| "stop".into()),
            });
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
    System {
        content: String,
    },
    User {
        content: String,
    },
    Assistant {
        content: Option<String>,
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
            Role::System => out_msgs.push(RequestMessage::System {
                content: m.content.clone(),
            }),
            Role::User => out_msgs.push(RequestMessage::User {
                content: m.content.clone(),
            }),
            Role::Assistant => {
                let content = if m.content.is_empty() && !m.tool_calls.is_empty() {
                    None
                } else {
                    Some(m.content.clone())
                };
                out_msgs.push(RequestMessage::Assistant {
                    content,
                    tool_calls: m
                        .tool_calls
                        .iter()
                        .map(|tc| RequestToolCall {
                            id: tc.id.clone(),
                            kind: "function",
                            function: RequestFunction {
                                name: tc.name.clone(),
                                arguments: tc.args.to_string(),
                            },
                        })
                        .collect(),
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
    #[serde(default)]
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
        let mut msgs = vec![Message::system("sys"), Message::user("hi")];
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

    #[test]
    fn build_request_serializes_null_content_when_assistant_has_tool_calls_and_empty_text() {
        let msgs = vec![Message::assistant(
            "",
            vec![ToolCall {
                id: "call_1".into(),
                name: "read".into(),
                args: serde_json::json!({"path": "src/main.rs"}),
            }],
        )];
        let body = build_request(&msgs, &[], "deepseek-chat", false);
        let json = serde_json::to_string(&body).unwrap();
        // Crucial for DeepSeek/Ollama: content MUST be present and null
        assert!(
            json.contains("\"content\":null"),
            "Expected 'content': null in JSON payload, but got: {json}"
        );
        assert!(json.contains("\"tool_calls\""));
    }

    #[test]
    fn chunk_tool_call_handles_missing_index() {
        let json = r#"{"id":"call_123","function":{"name":"read","arguments":"{}"}}"#;
        let parsed: ChunkToolCall =
            serde_json::from_str(json).expect("Should deserialize without index");
        assert_eq!(parsed.index, 0);
        assert_eq!(parsed.id.as_deref(), Some("call_123"));
    }

    #[test]
    fn validate_base_url_accepts_clean_https() {
        assert_eq!(
            validate_base_url("https://api.openai.com/v1").unwrap(),
            "https://api.openai.com/v1"
        );
    }

    #[test]
    fn validate_base_url_strips_trailing_slash() {
        assert_eq!(
            validate_base_url("https://api.openai.com/v1/").unwrap(),
            "https://api.openai.com/v1"
        );
    }

    #[test]
    fn validate_base_url_accepts_http_and_https() {
        assert!(validate_base_url("http://localhost:8080/v1").is_ok());
        assert!(validate_base_url("https://api.deepseek.com").is_ok());
    }

    #[test]
    fn validate_base_url_rejects_empty() {
        assert!(validate_base_url("").is_err());
        assert!(validate_base_url("   ").is_err());
    }

    #[test]
    fn validate_base_url_rejects_no_scheme() {
        let err = validate_base_url("api.openai.com/v1").unwrap_err();
        assert!(err.contains("URL") || err.contains("scheme"));
    }

    #[test]
    fn validate_base_url_rejects_wrong_scheme() {
        let err = validate_base_url("ftp://example.com").unwrap_err();
        assert!(err.contains("scheme") || err.contains("http"));
    }

    #[test]
    fn validate_base_url_rejects_no_host() {
        // "://no-scheme" lacks both a scheme and a host; url will
        // refuse to parse it at all.
        let err = validate_base_url("://no-scheme").unwrap_err();
        assert!(err.contains("URL") || err.contains("scheme"));
    }

    #[test]
    fn compat_checked_returns_clean_error_on_bad_url() {
        let err = compat_checked_for_test("not a url", "m", "k").unwrap_err();
        assert!(err.contains("URL") || err.contains("scheme"));
    }

    #[test]
    fn compat_checked_strips_trailing_slash() {
        let p = compat_checked_for_test("https://x.test/v1/", "m", "k").unwrap();
        assert!(!p.base_url.ends_with('/'));
    }

    fn compat_checked_for_test(url: &str, m: &str, k: &str) -> Result<OpenAiProvider, String> {
        OpenAiProvider::compat_checked(url, m.to_string(), k.to_string())
    }
}
