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

/// v0.4.22 (event 000118, fix 6 hardening): the wire-level
/// `chat_history` field arrives as `serde_json::Value` (because
/// the Tauri body is a generic `Value`); some entries may be
/// malformed (truncated JSON, missing role, wrong types). The
/// orchestrator should never crash on a single bad turn — it
/// should drop it and continue with the rest. This helper
/// performs the deserialization + filtering once, returning a
/// `Vec<Message>` that's safe to feed into the agent's history.
///
/// Rules (deliberately lenient):
///   * `body["chat_history"]` is `None` or not an array → empty Vec.
///   * Each entry is deserialized with `serde_json::from_value`.
///     On error → drop silently.
///   * Empty array → empty Vec.
///   * The result is returned in input order.
pub fn parse_chat_history(body: &serde_json::Value) -> Vec<Message> {
    body.get("chat_history")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|entry| {
                    let msg = serde_json::from_value::<Message>(entry.clone()).ok()?;
                    if msg.role == Role::Tool
                        && msg.tool_call_id.as_deref().unwrap_or("").is_empty()
                    {
                        return None;
                    }
                    Some(msg)
                })
                .collect()
        })
        .unwrap_or_default()
}

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

    // ── v0.4.22 (event 000118, fix 6 hardening): boundary
    // tests for parse_chat_history. The wire-level chat_history
    // can come from a frontend with bugs (truncated JSON, wrong
    // types, missing role) or a third-party replay. The
    // orchestrator must NEVER crash on a bad entry — just drop
    // it and continue with the rest.

    #[test]
    fn parse_chat_history_missing_field_is_empty() {
        let body = serde_json::json!({ "task": "x", "role": "agent:chief" });
        assert_eq!(parse_chat_history(&body).len(), 0);
    }

    #[test]
    fn parse_chat_history_null_is_empty() {
        let body = serde_json::json!({ "chat_history": null });
        assert_eq!(parse_chat_history(&body).len(), 0);
    }

    #[test]
    fn parse_chat_history_empty_array_is_empty() {
        let body = serde_json::json!({ "chat_history": [] });
        assert_eq!(parse_chat_history(&body).len(), 0);
    }

    #[test]
    fn parse_chat_history_wrong_type_treated_as_empty() {
        // chat_history is a string, not an array → no entries.
        let body = serde_json::json!({ "chat_history": "oops" });
        assert_eq!(parse_chat_history(&body).len(), 0);
    }

    #[test]
    fn parse_chat_history_drops_malformed_entries_keeps_good_ones() {
        let body = serde_json::json!({
            "chat_history": [
                { "role": "user", "content": "hello" },
                { "role": "user" },  // missing content
                { "content": "no role" },  // missing role
                { "role": "alien", "content": "x" },  // invalid role
                { "role": "user", "content": "world" },
            ]
        });
        let parsed = parse_chat_history(&body);
        assert_eq!(
            parsed.len(),
            2,
            "should keep only the 2 well-formed entries"
        );
        assert_eq!(parsed[0].role, Role::User);
        assert_eq!(parsed[0].content, "hello");
        assert_eq!(parsed[1].role, Role::User);
        assert_eq!(parsed[1].content, "world");
    }

    #[test]
    fn parse_chat_history_preserves_assistant_with_tool_calls() {
        // Frontend replays an assistant turn that had tool calls.
        let body = serde_json::json!({
            "chat_history": [
                { "role": "user", "content": "ls" },
                {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [{
                        "id": "c1",
                        "name": "bash",
                        "args": { "cmd": "ls" }
                    }]
                },
                { "role": "tool", "tool_call_id": "c1", "content": "ok" },
            ]
        });
        let parsed = parse_chat_history(&body);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[1].role, Role::Assistant);
        assert_eq!(parsed[1].tool_calls.len(), 1);
        assert_eq!(parsed[1].tool_calls[0].id, "c1");
        assert_eq!(parsed[2].role, Role::Tool);
        assert_eq!(parsed[2].tool_call_id.as_deref(), Some("c1"));
    }

    #[test]
    fn parse_chat_history_assistant_with_empty_tool_calls_keeps_empty_vec() {
        // serde skips empty tool_calls on the wire, so the field
        // is absent. The parser must default it to Vec::new().
        let body = serde_json::json!({
            "chat_history": [
                { "role": "assistant", "content": "no tools here" }
            ]
        });
        let parsed = parse_chat_history(&body);
        assert_eq!(parsed.len(), 1);
        assert!(parsed[0].tool_calls.is_empty());
    }

    #[test]
    fn parse_chat_history_system_role_is_preserved() {
        // We do NOT inject system messages via chat_history (the
        // orchestrator's system prompt is hard-coded), but if a
        // frontend ever sends one, we keep it (some providers
        // re-order). The point of this test: nothing in the
        // parser strips system entries.
        let body = serde_json::json!({
            "chat_history": [
                { "role": "system", "content": "be terse" },
                { "role": "user", "content": "hi" }
            ]
        });
        let parsed = parse_chat_history(&body);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].role, Role::System);
    }

    #[test]
    fn parse_chat_history_truncated_json_array_member_drops_that_member() {
        // Simulate a corrupted history item. serde_json::from_value
        // returns Err on a `null` object that doesn't have role.
        let body = serde_json::json!({
            "chat_history": [
                { "role": "user", "content": "first" },
                null,
                { "role": "user", "content": "third" }
            ]
        });
        let parsed = parse_chat_history(&body);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].content, "first");
        assert_eq!(parsed[1].content, "third");
    }

    #[test]
    fn parse_chat_history_order_preserved() {
        let body = serde_json::json!({
            "chat_history": [
                { "role": "user", "content": "a" },
                { "role": "assistant", "content": "b" },
                { "role": "user", "content": "c" },
                { "role": "assistant", "content": "d" }
            ]
        });
        let parsed = parse_chat_history(&body);
        let contents: Vec<&str> = parsed.iter().map(|m| m.content.as_str()).collect();
        assert_eq!(contents, vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn parse_chat_history_tool_call_id_required_for_tool_role() {
        // Tool role without tool_call_id is malformed → drop.
        let body = serde_json::json!({
            "chat_history": [
                { "role": "tool", "content": "no id" },  // malformed
                { "role": "tool", "tool_call_id": "ok", "content": "with id" }
            ]
        });
        let parsed = parse_chat_history(&body);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].tool_call_id.as_deref(), Some("ok"));
    }

    #[test]
    fn parse_chat_history_very_long_content_is_not_truncated() {
        // 1MB string — we don't enforce a length limit at parse
        // time; the agent loop caps via MAX_CHAT_HISTORY.
        let big = "x".repeat(1_000_000);
        let body = serde_json::json!({
            "chat_history": [
                { "role": "user", "content": big.clone() }
            ]
        });
        let parsed = parse_chat_history(&body);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].content.len(), 1_000_000);
    }
}
