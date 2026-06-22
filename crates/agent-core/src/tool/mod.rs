//! Built-in tools the agent can call.
//!
//! Every tool implements [`Tool`], which means it has a stable
//! name, a JSON-Schema for its arguments, and an async execute
//! method. The agent loop and the prompt engine talk to tools
//! only through this trait; nothing else in the crate cares
//! about specific tool implementations.

pub mod bash;
pub mod grep;
pub mod patch;
pub mod read;
pub mod write;

use async_trait::async_trait;
use std::collections::HashMap;
use thiserror::Error;

use crate::workspace::Workspace;

/// Errors a tool can return.
#[derive(Debug, Error)]
pub enum ToolError {
    /// The supplied `args` failed JSON-Schema validation.
    #[error("invalid arguments: {0}")]
    InvalidArgs(String),

    /// IO error during tool execution.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// The tool explicitly refused to run (e.g. dangerous bash
    /// pattern, patch that didn't apply).
    #[error("{0}")]
    Refused(String),

    /// Catch-all.
    #[error("{0}")]
    Other(String),
}

/// Outcome of a tool execution. The full string goes into the
/// LLM's history; a short `preview` is surfaced to the UI
/// timeline.
#[derive(Debug, Clone)]
pub struct ToolOutput {
    /// Full result, sent back to the model verbatim.
    pub content: String,
    /// Short summary for the UI (first ~200 chars, single-line).
    pub preview: String,
    /// Did the tool fail?
    pub is_error: bool,
}

impl ToolOutput {
    /// Wrap a successful result.
    pub fn ok(content: impl Into<String>) -> Self {
        let s = content.into();
        let preview = preview_of(&s);
        Self { content: s, preview, is_error: false }
    }

    /// Wrap an error result.
    pub fn err(content: impl Into<String>) -> Self {
        let s = content.into();
        let preview = preview_of(&s);
        Self { content: s, preview, is_error: true }
    }
}

fn preview_of(s: &str) -> String {
    let first_line = s.lines().next().unwrap_or("");
    let truncated: String = first_line.chars().take(200).collect();
    if first_line.chars().count() > 200 { format!("{truncated}…") } else { truncated }
}

/// Context handed to every tool invocation.
#[derive(Debug, Clone)]
pub struct ToolContext {
    /// Workspace the tool operates in (project root for read/write,
    /// CWD for bash).
    pub workspace: Workspace,
    /// Whether the agent has explicit user approval for
    /// potentially-destructive actions. Tools like `write` and
    /// `patch` may bypass some safety checks when this is true.
    pub approved: bool,
}

/// A tool the agent can call.
#[async_trait]
pub trait Tool: Send + Sync + std::fmt::Debug {
    /// Stable name; must match what the LLM emits in tool calls.
    fn name(&self) -> &'static str;

    /// Human description used in the system prompt.
    fn description(&self) -> &'static str;

    /// JSON-Schema describing the tool's arguments. Inlined into
    /// the system prompt so the model knows how to call it.
    fn schema(&self) -> serde_json::Value;

    /// Execute the tool.
    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError>;
}

/// A registry of tools, looked up by name.
///
/// The default registry ships the built-in tools; users (and
/// future plugins) can register additional ones.
#[derive(Debug, Default)]
pub struct ToolRegistry {
    tools: HashMap<&'static str, Box<dyn Tool>>,
}

impl ToolRegistry {
    /// Build a new registry pre-populated with all built-in tools.
    pub fn with_builtins() -> Self {
        let mut r = Self::default();
        r.register(Box::new(bash::BashTool));
        r.register(Box::new(read::ReadTool));
        r.register(Box::new(write::WriteTool));
        r.register(Box::new(patch::PatchTool));
        r.register(Box::new(grep::GrepTool));
        r
    }

    /// Add (or replace) a tool by name.
    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.insert(tool.name(), tool);
    }

    /// Look up a tool by name.
    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|b| b.as_ref())
    }

    /// List every tool's name + schema (for system prompt).
    pub fn schemas(&self) -> Vec<serde_json::Value> {
        let mut out: Vec<_> = self
            .tools
            .values()
            .map(|t| serde_json::json!({
                "type": "function",
                "function": {
                    "name": t.name(),
                    "description": t.description(),
                    "parameters": t.schema(),
                }
            }))
            .collect();
        out.sort_by(|a, b| {
            a["function"]["name"]
                .as_str()
                .unwrap_or("")
                .cmp(b["function"]["name"].as_str().unwrap_or(""))
        });
        out
    }

    /// Execute a tool by name.
    pub async fn execute(
        &self,
        name: &str,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let tool = self
            .get(name)
            .ok_or_else(|| ToolError::Other(format!("unknown tool: {name}")))?;
        tool.execute(args, ctx).await
    }

    /// Number of registered tools.
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// True if no tools are registered.
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_truncates() {
        let s = "a".repeat(500);
        let p = preview_of(&s);
        assert!(p.chars().count() <= 201); // 200 + ellipsis
    }

    #[test]
    fn output_ok_and_err() {
        let o = ToolOutput::ok("hi");
        assert!(!o.is_error);
        assert_eq!(o.content, "hi");
        let e = ToolOutput::err("nope");
        assert!(e.is_error);
    }
}