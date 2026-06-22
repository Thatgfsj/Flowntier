//! Conversation message types shared between providers and tools.
//!
//! This is the internal Rust representation. Provider-specific
//! formats (OpenAI's `messages[]`, Anthropic's `messages[]` +
//! separate `system` field, Gemini's `contents[]`) are converted
//! to/from this at the provider boundary.

use serde::{Deserialize, Serialize};

/// Role of a message author.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// System prompt (instructions to the model).
    System,
    /// User input (task envelope, follow-up message, etc.).
    User,
    /// Assistant (model) response — may include text and tool calls.
    Assistant,
    /// Tool result (the output of a single tool invocation).
    Tool,
}

/// A chat message in the conversation history.
///
/// Tool calls are attached to the **assistant** message that
/// produced them; tool results are returned as a separate
/// [`Message::Tool`] message whose `tool_call_id` references
/// the original `ToolCall.id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Role of the author.
    pub role: Role,
    /// Text content (may be empty when the message is tool-only).
    pub content: String,
    /// Tool calls emitted by the assistant (assistant role only).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// Reference to the originating tool call (tool role only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Message {
    /// System-prompt constructor.
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }

    /// User-prompt constructor.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }

    /// Assistant constructor with optional tool calls.
    pub fn assistant(content: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            tool_calls,
            tool_call_id: None,
        }
    }

    /// Tool-result constructor.
    pub fn tool(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: Some(tool_call_id.into()),
        }
    }
}

/// Convenience alias for the common chat-message shape.
pub type ChatMessage = Message;

/// A request the assistant made to invoke a tool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolCall {
    /// Stable id, assigned by the provider. Used to match the
    /// returned tool result back to this call.
    pub id: String,
    /// Tool name (must match a registered [`crate::tool::Tool::name`]).
    pub name: String,
    /// JSON arguments, already validated against the tool's schema.
    pub args: serde_json::Value,
}

/// The result of executing a tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// The id of the originating tool call.
    pub tool_call_id: String,
    /// Full output (stored in history verbatim; may be large).
    pub content: String,
    /// Whether the tool execution succeeded.
    pub is_error: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_set_role() {
        assert_eq!(Message::system("x").role, Role::System);
        assert_eq!(Message::user("x").role, Role::User);
        let a = Message::assistant("x", vec![]);
        assert_eq!(a.role, Role::Assistant);
        let t = Message::tool("id1", "out");
        assert_eq!(t.role, Role::Tool);
        assert_eq!(t.tool_call_id.as_deref(), Some("id1"));
    }

    #[test]
    fn json_roundtrip() {
        let m = Message::assistant(
            "hi",
            vec![ToolCall {
                id: "c1".into(),
                name: "bash".into(),
                args: serde_json::json!({"cmd": "ls"}),
            }],
        );
        let s = serde_json::to_string(&m).unwrap();
        let back: Message = serde_json::from_str(&s).unwrap();
        assert_eq!(back.role, Role::Assistant);
        assert_eq!(back.tool_calls.len(), 1);
        assert_eq!(back.tool_calls[0].id, "c1");
    }
}