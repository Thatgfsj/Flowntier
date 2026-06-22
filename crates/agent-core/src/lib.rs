//! Embedded agent core for Agent Company OS (v0.3+).
//!
//! This crate replaces the previous two-sidecar architecture
//! (Python FastAPI runtime + Claude Code CLI over portable-pty)
//! with a single in-process Rust implementation.
//!
//! ## What lives here
//!
//! - [`provider`] — the `Provider` trait + first-party impls
//!   (OpenAI, Anthropic, Gemini, OpenAI-compat). Streams chat
//!   completions via SSE.
//! - [`tool`] — the `Tool` trait + built-in tools (bash, read,
//!   write, patch, grep, glob). Tools are invoked by the agent
//!   loop and their outputs are streamed to the UI.
//! - [`loop_`] — the agent loop itself: stream LLM → detect
//!   tool calls → execute → push results into history → repeat.
//! - [`prompt`] — role-specific system prompts and the
//!   placeholder-substitution template engine.
//! - [`context`] — token counting + truncation + summarization.
//! - [`event`] — the `AgentEvent` enum (text delta, tool started,
//!   tool finished, file diff, etc.) that the rest of ACO
//!   subscribes to.
//!
//! ## What does NOT live here
//!
//! - Tauri glue (`crates/tauri-core`)
//! - SQLite (`crates/storage`)
//! - Workflow state machine (`crates/workflow`)  — future
//! - Configuration parsing (`crates/config`)
//!
//! See `docs/ARCHITECTURE.md` §4 for the full design.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod context;
pub mod event;
pub mod loop_;
pub mod message;
pub mod prompt;
pub mod provider;
pub mod tool;
pub mod workspace;

pub use event::AgentEvent;
pub use loop_::{Agent, AgentConfig};
pub use message::{ChatMessage, Message, Role, ToolCall, ToolResult};
pub use provider::{Provider, ProviderError, StreamChunk};
pub use tool::{Tool, ToolContext, ToolError, ToolOutput, ToolRegistry};
pub use workspace::Workspace;

/// Crate-wide error type. Wraps provider / tool / IO / serialization
/// failures so callers can match on the variant.
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    /// Provider returned an error or its stream was malformed.
    #[error("provider error: {0}")]
    Provider(#[from] ProviderError),

    /// Tool execution failed.
    #[error("tool error: {0}")]
    Tool(#[from] ToolError),

    /// IO error (file read / write / subprocess).
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON (de)serialization failed.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    /// Context budget exhausted and summarization couldn't free
    /// enough room.
    #[error("context budget exhausted ({used} / {budget} tokens)")]
    ContextBudgetExhausted {
        /// Tokens currently used.
        used: usize,
        /// Token budget.
        budget: usize,
    },

    /// The agent loop hit a configured max-iteration cap.
    #[error("agent loop hit max iterations ({0})")]
    MaxIterationsReached(usize),

    /// Catch-all.
    #[error("{0}")]
    Other(String),
}

impl From<anyhow::Error> for AgentError {
    fn from(e: anyhow::Error) -> Self {
        Self::Other(e.to_string())
    }
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, AgentError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_conversions_compile() {
        let _: AgentError = std::io::Error::other("x").into();
        let _: AgentError = serde_json::from_str::<i32>("nope").unwrap_err().into();
    }
}