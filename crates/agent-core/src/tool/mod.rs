//! Built-in tools the agent can call.
//!
//! Every tool implements [`Tool`], which means it has a stable
//! name, a JSON-Schema for its arguments, and an async execute
//! method. The agent loop and the prompt engine talk to tools
//! only through this trait; nothing else in the crate cares
//! about specific tool implementations.

pub mod bash;
pub mod grep;
pub mod nwt;
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
    /// Fine-grained capabilities. Default = everything on.
    /// Construct non-default contexts with [`Capabilities::read_only`],
    /// [`Capabilities::network_off`], etc.
    pub capabilities: Capabilities,
    /// Cancellation token. Tools should poll this between heavy
    /// steps; the `bash` tool already races its subprocess against
    /// `cancel.cancelled()` via the provider's SSE stream. The
    /// token here lets tools that take longer than a single
    /// subprocess observe user-cancellation.
    pub cancel: Option<tokio_util::sync::CancellationToken>,
}

impl Default for ToolContext {
    fn default() -> Self {
        Self {
            workspace: Workspace::default(),
            approved: true,
            capabilities: Capabilities::default(),
            cancel: None,
        }
    }
}

impl ToolContext {
    /// New context with all capabilities enabled. Use this when
    /// the agent is running in "trusted" mode (e.g. dev with the
    /// user watching).
    pub fn new(workspace: Workspace) -> Self {
        Self {
            workspace,
            approved: true,
            capabilities: Capabilities::default(),
            cancel: None,
        }
    }
}

/// Per-tool permission flags.
///
/// All flags default to `true` (everything allowed). Construct a
/// stricter context when needed:
///
/// ```ignore
/// let caps = Capabilities::read_only();
/// let ctx = ToolContext { workspace, approved: true, capabilities: caps };
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// `read` tool, and any other read-style ops.
    pub read: bool,
    /// `write`, `patch` — files may be modified or created.
    pub write: bool,
    /// `bash` — child processes may be spawned.
    pub bash: bool,
    /// Outbound network from `bash` is allowed (curl, wget, npm
    /// install, …). Combined with `bash=false`, this is a no-op.
    pub network: bool,
}

impl Default for Capabilities {
    fn default() -> Self {
        Self { read: true, write: true, bash: true, network: true }
    }
}

impl Capabilities {
    /// File reading + `bash` for inspection commands, but no
    /// modification or outbound network.
    pub fn read_only() -> Self {
        Self { read: true, write: false, bash: true, network: false }
    }
    /// Completely locked down: nothing but `read`. Useful for
    /// the "inspect this codebase" mode where the user wants
    /// the agent to plan but not touch anything yet.
    pub fn no_modify() -> Self {
        Self { read: true, write: false, bash: false, network: false }
    }
    /// No outbound network from `bash`, but local file ops and
    /// local subprocesses are fine.
    pub fn network_off() -> Self {
        Self { read: true, write: true, bash: true, network: false }
    }
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
        // v0.4.22 (event 000091 fix #1): the nwt_log tool was
        // implemented in `tool/nwt.rs` (436 lines) but never
        // registered. Every role's system prompt (NWT_INSTRUCTION
        // in prompt/mod.rs) tells the model to call nwt_log —
        // without registration, the model would call a non-
        // existent tool, fail 50 iterations, and burn the
        // max_iterations budget on every run. Registering fixes
        // the silent MaxIterationsReached hang.
        r.register(Box::new(nwt::NwtLogTool));
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

    #[test]
    fn capabilities_read_only_disables_write() {
        let caps = Capabilities::read_only();
        assert!(caps.read);
        assert!(!caps.write);
        assert!(caps.bash);
        assert!(!caps.network);
    }

    #[test]
    fn capabilities_no_modify_disables_bash() {
        let caps = Capabilities::no_modify();
        assert!(caps.read);
        assert!(!caps.write);
        assert!(!caps.bash);
        assert!(!caps.network);
    }
}