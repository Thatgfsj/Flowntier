//! Workspace-level `flowntier.toml` (v0.4.36).
//!
//! The per-project config surface for the runtime. This is NOT the
//! app-level `flowntier.toml` that `crates/config` models (the
//! app-level struct is currently only constructed in code by the
//! Tauri shell); this module parses a small, purpose-built subset
//! straight from the workspace root:
//!
//! ```toml
//! # <workspace>/flowntier.toml
//! [permissions]
//! mode = "auto"            # "auto" (default) | "readonly"
//!
//! [mcp.servers.filesystem]
//! command = "npx.cmd"
//! args    = ["-y", "@modelcontextprotocol/server-filesystem", "."]
//! enabled = true           # default
//!
//! [mcp.servers.git.env]
//! GIT_DIR = "..."
//! ```
//!
//! Rationale (from the v0.4.36 debate): MCP servers are project-
//! scoped in every mainstream harness, so the config lives in the
//! project; every field has a `#[serde(default)]` so a file that
//! only sets `[permissions]` parses fine, and a missing file yields
//! defaults instead of an error: agents must keep running when the
//! config is absent or half-broken.

use agent_core::mcp::McpServerConfig;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// Parsed workspace `flowntier.toml`.
// Struct-level serde(default): a file that only sets one section
// (e.g. just `[permissions]`) must parse — missing sections are
// filled from Default.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspaceToml {
    /// `[mcp]` section (server list).
    pub mcp: McpSection,
    /// `[permissions]` section.
    pub permissions: PermissionsSection,
}

/// `[mcp]` section.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpSection {
    /// `[mcp.servers.<name>]` entries, keyed by server name.
    #[serde(default)]
    pub servers: BTreeMap<String, McpServerEntry>,
}

/// One `[mcp.servers.<name>]` entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServerEntry {
    /// Executable (`.cmd`/`.bat` routed via `cmd /C` on Windows).
    pub command: String,
    /// argv tail.
    #[serde(default)]
    pub args: Vec<String>,
    /// Extra child env.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Disabled servers are skipped entirely (default true).
    #[serde(default = "crate::workspace_toml::default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

/// Tool permission mode. `"readonly"` maps to
/// `Capabilities::no_modify()` + registry-level stripping of every
/// write-class tool (bash included, since a readonly agent that could
/// run `echo x > f` wouldn't be readonly).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionMode {
    /// Full capabilities (default).
    #[default]
    Auto,
    /// No writes, no bash, no network, no MCP tools.
    ReadOnly,
}

/// `[permissions]` section.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionsSection {
    /// `mode = "auto" | "readonly"` (default auto).
    #[serde(default)]
    pub mode: PermissionMode,
}

impl WorkspaceToml {
    /// Parse an in-memory toml fragment. Also the seam used by the
    /// debug probe and tests.
    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    /// Load from `<root>/flowntier.toml`. Missing file → defaults.
    /// Broken file → defaults + a `warn!` (never an error: the
    /// config must not take agent runs down).
    pub fn load(root: &Path) -> Self {
        let path = root.join("flowntier.toml");
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => return Self::default(),
        };
        match Self::parse(&text) {
            Ok(cfg) => cfg,
            Err(e) => {
                tracing::warn!("ignoring invalid {}: {e}", path.display());
                Self::default()
            }
        }
    }

    /// Enabled servers converted into agent-core's config shape,
    /// deterministic (BTreeMap) order.
    pub fn enabled_mcp_servers(&self) -> Vec<McpServerConfig> {
        self.mcp
            .servers
            .iter()
            .filter(|(_, e)| e.enabled && !e.command.trim().is_empty())
            .map(|(name, e)| McpServerConfig {
                name: name.clone(),
                command: e.command.clone(),
                args: e.args.clone(),
                env: e.env.clone(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_yields_defaults() {
        let dir = std::env::temp_dir().join(format!("ft-toml-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = WorkspaceToml::load(&dir);
        assert_eq!(cfg.permissions.mode, PermissionMode::Auto);
        assert!(cfg.mcp.servers.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_mcp_servers_and_permissions() {
        let dir = std::env::temp_dir().join(format!("ft-toml2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("flowntier.toml"),
            r#"
[permissions]
mode = "readonly"

[mcp.servers.filesystem]
command = "npx.cmd"
args = ["-y", "@modelcontextprotocol/server-filesystem", "."]

[mcp.servers.git]
command = "uvx"
args = ["mcp-server-git"]
enabled = false
"#,
        )
        .unwrap();
        let cfg = WorkspaceToml::load(&dir);
        assert_eq!(cfg.permissions.mode, PermissionMode::ReadOnly);
        assert_eq!(cfg.mcp.servers.len(), 2);
        let enabled = cfg.enabled_mcp_servers();
        assert_eq!(enabled.len(), 1, "disabled servers are filtered");
        assert_eq!(enabled[0].name, "filesystem");
        assert_eq!(enabled[0].command, "npx.cmd");
        assert_eq!(enabled[0].args.len(), 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn broken_toml_falls_back_to_defaults() {
        let dir = std::env::temp_dir().join(format!("ft-toml3-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("flowntier.toml"), "[permissions\nmode = ").unwrap();
        let cfg = WorkspaceToml::load(&dir);
        assert_eq!(cfg.permissions.mode, PermissionMode::Auto);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn partial_file_only_permissions_is_fine() {
        let dir = std::env::temp_dir().join(format!("ft-toml4-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("flowntier.toml"),
            "[permissions]\nmode = \"readonly\"\n",
        )
        .unwrap();
        let cfg = WorkspaceToml::load(&dir);
        assert_eq!(cfg.permissions.mode, PermissionMode::ReadOnly);
        assert!(cfg.mcp.servers.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
