//! Provider trait + shared types.
//!
//! Every LLM provider (OpenAI, Anthropic, Gemini, OpenAI-compat
//! relays) implements [`Provider`]. The agent loop only depends on
//! this trait; provider-specific wire formats are hidden.

use async_trait::async_trait;
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

use crate::message::{Message, ToolCall};

pub mod anthropic;
pub mod openai;

/// One chunk of a streaming chat completion.
///
/// Provider implementations translate their native SSE event
/// formats into this enum.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamChunk {
    /// Incremental text fragment. Multiple consecutive `Text`
    /// chunks concatenate into the full message.
    Text {
        /// Fragment.
        delta: String,
    },
    /// A complete tool call. Providers that stream tool-call
    /// deltas (OpenAI) coalesce them into a single `ToolUse`
    /// before yielding.
    ToolUse {
        /// The fully-formed tool call.
        call: ToolCall,
    },
    /// Provider is done. The stream terminates after this chunk.
    Done {
        /// Reason the stream ended (`stop`, `tool_calls`,
        /// `length`, etc.).
        reason: String,
    },
}

/// Errors a provider can return.
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    /// HTTP transport error.
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    /// SSE stream was malformed (couldn't parse a chunk).
    #[error("malformed SSE stream: {0}")]
    MalformedStream(String),

    /// The provider returned a non-2xx status with an error body.
    #[error("provider returned {status}: {body}")]
    Api {
        /// HTTP status code.
        status: u16,
        /// Error body (truncated).
        body: String,
    },

    /// Caller asked for a provider/model that this impl doesn't
    /// support.
    #[error("unsupported: {0}")]
    Unsupported(String),

    /// Catch-all.
    #[error("{0}")]
    Other(String),
}

/// Type alias for the streamed result of [`Provider::stream_chat`].
pub type ChatStream = Pin<Box<dyn Stream<Item = Result<StreamChunk, ProviderError>> + Send>>;

/// A chat-completion provider.
///
/// Implementations: [`openai::OpenAiProvider`] (covers OpenAI and
/// any OpenAI-compat endpoint — DeepSeek, Moonshot, Ollama, etc.),
/// [`anthropic::AnthropicProvider`].
#[async_trait]
pub trait Provider: Send + Sync + std::fmt::Debug {
    /// Provider id (e.g. `"openai"`, `"anthropic"`,
    /// `"openai_compat"`). Matches the id used in
    /// `provider-presets`.
    fn id(&self) -> &'static str;

    /// Model id (e.g. `"claude-sonnet-4.5"`, `"gpt-4o"`).
    fn model_id(&self) -> &str;

    /// Stream a chat completion.
    ///
    /// `messages` is the full conversation history (system +
    /// user + assistant + tool). `tools` is the list of tool
    /// schemas the provider should expose to the model — empty
    /// for pure chat. `cancel` is checked between stream polls
    /// so the agent loop can abort.
    async fn stream_chat(
        &self,
        messages: &[Message],
        tools: &[serde_json::Value],
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<ChatStream, ProviderError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_serde_roundtrip() {
        let c = StreamChunk::Text {
            delta: "hi".into(),
        };
        let s = serde_json::to_string(&c).unwrap();
        assert!(s.contains("\"text\""));
        let back: StreamChunk = serde_json::from_str(&s).unwrap();
        match back {
            StreamChunk::Text { delta } => assert_eq!(delta, "hi"),
            _ => panic!("wrong variant"),
        }
    }
}