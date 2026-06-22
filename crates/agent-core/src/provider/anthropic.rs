//! Anthropic Messages API provider.
//!
//! Covers the entire Claude 3.x / 4.x family. Wire format:
//! <https://docs.anthropic.com/en/api/messages-streaming>
//!
//! Differences from OpenAI's wire format:
//! - System prompt is a top-level field, not a message.
//! - Tool calls and text live as content blocks, not separate fields.
//! - SSE events are typed (`message_start`, `content_block_*`,
//!   `message_delta`, `message_stop`) rather than per-choice
//!   delta frames.

use async_trait::async_trait;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use super::{ChatStream, Provider, ProviderError, StreamChunk};
use crate::message::{Message, Role, ToolCall};

/// An Anthropic Messages provider.
#[derive(Debug, Clone)]
pub struct AnthropicProvider {
    /// Model id, e.g. `claude-sonnet-4.5`.
    pub model: String,
    /// Bearer token (Anthropic-style: also accepts `x-api-key`).
    pub api_key: String,
    /// `anthropic-version` header (defaults to `2023-06-01`).
    pub api_version: String,
    /// Optional custom base URL (for proxies / Bedrock-like relays).
    pub base_url: Option<String>,
}

impl AnthropicProvider {
    /// Build a provider pointed at the public Anthropic API.
    pub fn new(model: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            api_key: api_key.into(),
            api_version: "2023-06-01".into(),
            base_url: None,
        }
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    fn id(&self) -> &'static str {
        "anthropic"
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
        let (system, msgs) = split_system(messages);

        let body = MessagesRequest {
            model: &self.model,
            max_tokens: 8192,
            system: &system,
            messages: &msgs,
            tools,
            stream: true,
        };

        let url = format!(
            "{}/v1/messages",
            self.base_url
                .as_deref()
                .unwrap_or("https://api.anthropic.com")
                .trim_end_matches('/')
        );
        let resp = reqwest::Client::new()
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", &self.api_version)
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

        let byte_stream = resp.bytes_stream();
        let sse = eventsource_stream::EventStream::new(byte_stream);
        let model = self.model.clone();

        // Anthropic streams content blocks. We need to track
        // partial tool-input JSON across `input_json_delta` events
        // and emit `ToolUse` chunks only when the block closes
        // (`content_block_stop`).
        let stream = async_stream::stream! {
            let mut current_block: Option<PartialBlock> = None;
            let mut pending_calls: Vec<ToolCall> = Vec::new();
            let mut stop_reason: Option<String> = None;

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
                        match event.event.as_str() {
                            "content_block_start" => {
                                #[derive(Deserialize)]
                                struct Start {
                                    content_block: StartBlock,
                                }
                                #[derive(Deserialize)]
                                #[serde(tag = "type", rename_all = "snake_case")]
                                enum StartBlock {
                                    Text {},
                                    ToolUse { id: String, name: String },
                                }
                                if let Ok(s) = serde_json::from_str::<Start>(&event.data) {
                                    match s.content_block {
                                        StartBlock::Text {} => {
                                            current_block = Some(PartialBlock::Text);
                                        }
                                        StartBlock::ToolUse { id, name } => {
                                            current_block = Some(PartialBlock::Tool {
                                                id,
                                                name,
                                                args: String::new(),
                                            });
                                        }
                                    }
                                }
                            }
                            "content_block_delta" => {
                                #[derive(Deserialize)]
                                struct Delta {
                                    delta: DeltaInner,
                                }
                                #[derive(Deserialize)]
                                #[serde(tag = "type", rename_all = "snake_case")]
                                enum DeltaInner {
                                    TextDelta { text: String },
                                    InputJsonDelta { partial_json: String },
                                }
                                if let Ok(d) = serde_json::from_str::<Delta>(&event.data) {
                                    match (current_block.as_mut(), d.delta) {
                                        (Some(PartialBlock::Text), DeltaInner::TextDelta { text }) => {
                                            if !text.is_empty() {
                                                yield Ok(StreamChunk::Text { delta: text });
                                            }
                                        }
                                        (Some(PartialBlock::Tool { args, .. }), DeltaInner::InputJsonDelta { partial_json }) => {
                                            args.push_str(&partial_json);
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            "content_block_stop" => {
                                if let Some(PartialBlock::Tool { id, name, args }) = current_block.take() {
                                    let parsed = if args.is_empty() {
                                        serde_json::json!({})
                                    } else {
                                        serde_json::from_str(&args).unwrap_or(serde_json::json!({}))
                                    };
                                    pending_calls.push(ToolCall {
                                        id: if id.is_empty() {
                                            format!("toolu_{}", ulid::Ulid::new())
                                        } else { id },
                                        name,
                                        args: parsed,
                                    });
                                }
                            }
                            "message_delta" => {
                                #[derive(Deserialize)]
                                struct Md { delta: MdInner }
                                #[derive(Deserialize)]
                                struct MdInner { #[serde(default)] stop_reason: Option<String> }
                                if let Ok(m) = serde_json::from_str::<Md>(&event.data) {
                                    if let Some(r) = m.delta.stop_reason { stop_reason = Some(r); }
                                }
                            }
                            "message_stop" => {
                                for call in pending_calls.drain(..) {
                                    yield Ok(StreamChunk::ToolUse { call });
                                }
                                yield Ok(StreamChunk::Done {
                                    reason: stop_reason.clone().unwrap_or_else(|| "end_turn".into()),
                                });
                                break;
                            }
                            "ping" | "message_start" => {}
                            other => {
                                tracing::debug!(event = %other, "ignoring Anthropic SSE event");
                            }
                        }
                    }
                }
            }
            // Suppress unused warning when stream is cancelled before message_stop.
            let _ = model;
        };

        Ok(Box::pin(stream))
    }
}

// ── Wire types ────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct MessagesRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    system: &'a str,
    messages: &'a [AnthropicMsg],
    tools: &'a [serde_json::Value],
    stream: bool,
}

#[derive(Debug, Serialize)]
#[serde(tag = "role", rename_all = "lowercase")]
enum AnthropicMsg {
    User { content: String },
    Assistant { content: Vec<AssistantBlock> },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AssistantBlock {
    Text { text: String },
    ToolUse { id: String, name: String, input: serde_json::Value },
}

fn split_system(messages: &[Message]) -> (String, Vec<AnthropicMsg>) {
    let mut system = String::new();
    let mut msgs = Vec::with_capacity(messages.len());
    for m in messages {
        match m.role {
            Role::System => {
                if !system.is_empty() { system.push('\n'); }
                system.push_str(&m.content);
            }
            Role::User => msgs.push(AnthropicMsg::User { content: m.content.clone() }),
            Role::Assistant => {
                let mut blocks = Vec::new();
                if !m.content.is_empty() {
                    blocks.push(AssistantBlock::Text { text: m.content.clone() });
                }
                for tc in &m.tool_calls {
                    blocks.push(AssistantBlock::ToolUse {
                        id: tc.id.clone(),
                        name: tc.name.clone(),
                        input: tc.args.clone(),
                    });
                }
                msgs.push(AnthropicMsg::Assistant { content: blocks });
            }
            Role::Tool => {
                // Anthropic: tool results are *user* messages with
                // a tool_result content block. We approximate by
                // emitting them as user-role text. (Better impl
                // would emit a typed block; kept simple here.)
                msgs.push(AnthropicMsg::User {
                    content: format!("[tool_result id={}] {}", m.tool_call_id.as_deref().unwrap_or(""), m.content),
                });
            }
        }
    }
    (system, msgs)
}

enum PartialBlock {
    Text,
    Tool { id: String, name: String, args: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_system_pulls_out_system_role() {
        let msgs = vec![
            Message::system("sys"),
            Message::user("hi"),
            Message::assistant("ok", vec![]),
        ];
        let (sys, rest) = split_system(&msgs);
        assert_eq!(sys, "sys");
        assert_eq!(rest.len(), 2);
    }
}