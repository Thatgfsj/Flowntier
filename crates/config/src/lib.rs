//! Configuration loading for ACO.
//!
//! Reads `aco.toml`, `providers.toml`, and `router.toml` from the
//! filesystem, validates them, and exposes typed structs.
//!
//! **No secrets live in these files.** API keys come from environment
//! variables (see `docs/SECURITY.md` §2).
//!
//! See `docs/CONFIG.md` for the full schema.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Top-level runtime config. See `docs/CONFIG.md` §3.
///
/// Note: derives `PartialEq` but not `Eq` because some nested
/// sections (`LoggingSection`, `ModelSpec`) contain `f32`/`f64`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AcoConfig {
    /// App-wide settings.
    pub app: AppSection,
    /// Workflow budgets and limits.
    pub workflow: WorkflowSection,
    /// UI preferences.
    pub ui: UiSection,
    /// Logging config.
    pub logging: LoggingSection,
    /// Storage config.
    pub storage: StorageSection,
    /// Security config.
    pub security: SecuritySection,
}

/// App-wide settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppSection {
    /// Data directory path (may contain `~`).
    pub data_dir: String,
    /// Log level.
    pub log_level: LogLevel,
    /// Theme (dark/light).
    pub theme: Theme,
    /// Enable auto-update.
    pub auto_update: bool,
    /// Enable telemetry (always false in v0.1).
    pub telemetry: bool,
}

/// Workflow budgets and limits. See `docs/CONFIG.md` §3.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowSection {
    /// Max plan revisions before FAILED.
    pub max_plan_revisions: u32,
    /// Max repair loops per task before escalation.
    pub max_repair_loops: u32,
    /// Max concurrent workers.
    pub max_parallel_workers: u32,
    /// Max total tokens per workflow.
    pub max_total_tokens: u64,
    /// Max wallclock seconds per workflow.
    pub max_wallclock_secs: u64,
    /// Max seconds waiting on a user query.
    pub max_user_query_wait: u64,
}

/// UI preferences. See `docs/CONFIG.md` §3.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UiSection {
    /// Show token-level stream (default off).
    pub show_token_stream: bool,
    /// Show the bottom console.
    pub show_console: bool,
    /// Console height in pixels.
    pub console_height_px: u32,
    /// Timeline position.
    pub timeline_position: TimelinePosition,
}

/// Logging config.
///
/// Note: derives `PartialEq` but not `Eq` because `sample_console`
/// and `sample_events` are `f32`, which doesn't implement `Eq`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LoggingSection {
    /// Patterns to redact from logs.
    pub redact: Vec<String>,
    /// Format (`json` or `pretty`).
    pub format: LogFormat,
    /// Sampling rate for console (0.0 - 1.0).
    pub sample_console: f32,
    /// Sampling rate for events (0.0 - 1.0).
    pub sample_events: f32,
}

/// Storage config.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StorageSection {
    /// Days to retain `usage` rows.
    pub retention_usage_days: u32,
    /// Days to retain `prompts` rows.
    pub retention_prompts_days: u32,
    /// Backup directory.
    pub backup_dir: String,
    /// Daily backups to keep.
    pub backup_daily_keep: u32,
    /// Weekly backups to keep.
    pub backup_weekly_keep: u32,
}

/// Security config.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecuritySection {
    /// Allow plugins marked `unrestricted = true`.
    pub allow_unrestricted_plugins: bool,
    /// Confirm before writes outside the workspace.
    pub confirm_external_writes: bool,
    /// Require plugin signatures.
    pub require_signature_for_plugins: bool,
}

/// Log level. See `docs/CONFIG.md` §3.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    /// Trace.
    Trace,
    /// Debug.
    Debug,
    /// Info.
    Info,
    /// Warn.
    Warn,
    /// Error.
    Error,
}

/// Theme. Light theme is v0.2; v0.1 ships dark only.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    /// Dark theme (default).
    Dark,
    /// Light theme (v0.2).
    Light,
}

/// Timeline position.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TimelinePosition {
    /// Bottom of the window.
    Bottom,
    /// Top of the window.
    Top,
}

/// Log format.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    /// JSON.
    Json,
    /// Pretty-printed.
    Pretty,
}

/// Provider list. See `docs/PROVIDER_SPEC.md` §5.2 and `docs/CONFIG.md` §4.
///
/// Note: derives `PartialEq` but not `Eq` because `ProviderEntry`
/// contains `ModelSpec` which has `f64` fields.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ProvidersConfig {
    /// Providers keyed by id.
    #[serde(default)]
    pub providers: std::collections::BTreeMap<String, ProviderEntry>,
}

/// One provider entry.
///
/// Note: derives `PartialEq` but not `Eq` because `models` contains
/// `ModelSpec` which has `f64` fields.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderEntry {
    /// Provider kind (`anthropic` | `openai` | `google` | `openai_compat` | ...).
    #[serde(rename = "type")]
    pub kind: String,
    /// API base URL.
    pub base_url: String,
    /// Env var holding the API key.
    pub api_key_env: String,
    /// Whether this provider is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Model specs.
    #[serde(default)]
    pub models: std::collections::BTreeMap<String, ModelSpec>,
}

fn default_true() -> bool {
    true
}

/// Model spec. See `docs/PROVIDER_SPEC.md` §4.
///
/// Note: derives `PartialEq` but not `Eq` because `input_cost_mtok`
/// and `output_cost_mtok` are `f64`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelSpec {
    /// Human-readable name.
    pub display_name: String,
    /// Context window in tokens.
    pub context_window: u32,
    /// Max output tokens.
    pub max_output_tokens: u32,
    /// Input cost per million tokens (USD).
    pub input_cost_mtok: f64,
    /// Output cost per million tokens (USD).
    pub output_cost_mtok: f64,
    /// Capability list.
    pub capabilities: Vec<String>,
}

/// Model router. See `docs/PROVIDER_SPEC.md` §5.3.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RouterConfig {
    /// Default model per role.
    #[serde(default)]
    pub defaults: std::collections::BTreeMap<String, String>,
    /// Fallback chains per role.
    #[serde(default)]
    pub fallback: std::collections::BTreeMap<String, FallbackChain>,
    /// Per-task model pin overrides.
    #[serde(default)]
    pub overrides: std::collections::BTreeMap<String, String>,
}

/// One fallback chain.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FallbackChain {
    /// Ordered list of `provider:model` references.
    pub chain: Vec<String>,
}

/// Errors that may occur while loading config.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// I/O error.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// TOML parse error.
    #[error("toml parse error in {path}: {source}")]
    Toml {
        /// Path that failed.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: Box<toml::de::Error>,
    },
    /// YAML parse error.
    #[error("yaml parse error in {path}: {source}")]
    Yaml {
        /// Path that failed.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: Box<serde_yaml::Error>,
    },
    /// Config refers to an unknown provider.
    #[error("router references unknown provider: {0}")]
    UnknownProvider(String),
}

/// Load `aco.toml` from the given path.
pub fn load_aco_config(path: &Path) -> Result<AcoConfig, ConfigError> {
    let text = std::fs::read_to_string(path)?;
    let cfg: AcoConfig = toml::from_str(&text).map_err(|source| ConfigError::Toml {
        path: path.to_path_buf(),
        source: Box::new(source),
    })?;
    Ok(cfg)
}

/// Load `providers.toml` from the given path.
pub fn load_providers_config(path: &Path) -> Result<ProvidersConfig, ConfigError> {
    let text = std::fs::read_to_string(path)?;
    let cfg: ProvidersConfig = toml::from_str(&text).map_err(|source| ConfigError::Toml {
        path: path.to_path_buf(),
        source: Box::new(source),
    })?;
    Ok(cfg)
}

/// Load `router.toml` from the given path.
pub fn load_router_config(path: &Path) -> Result<RouterConfig, ConfigError> {
    let text = std::fs::read_to_string(path)?;
    let cfg: RouterConfig = toml::from_str(&text).map_err(|source| ConfigError::Toml {
        path: path.to_path_buf(),
        source: Box::new(source),
    })?;
    Ok(cfg)
}

/// Default user config directory.
///
/// Honors the `aco` → `flowntier` rename (2026-06-24). On first
/// call after upgrading, if a legacy `aco/` directory exists at the
/// same parent and the new `flowntier/` directory does not yet, the
/// legacy directory is renamed in place. The migration is logged
/// once and is best-effort: an `aco/` dir owned by another process
/// or with unreadable permissions is left untouched and the new
/// directory is returned.
pub fn default_user_config_dir() -> Option<PathBuf> {
    let new = dirs::config_dir().map(|p| p.join("flowntier"));
    if let Some(dir) = &new {
        if !dir.exists() {
            if let Some(legacy) = dirs::config_dir().map(|p| p.join("aco")) {
                if legacy.exists() {
                    match std::fs::rename(&legacy, dir) {
                        Ok(()) => eprintln!(
                            "[flowntier] migrated config dir: {} -> {}",
                            legacy.display(),
                            dir.display()
                        ),
                        Err(e) => eprintln!(
                            "[flowntier] could not migrate legacy config dir {} ({}); using fresh {}",
                            legacy.display(),
                            e,
                            dir.display()
                        ),
                    }
                }
            }
        }
    }
    new
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_aco_toml() {
        let toml = r#"
[app]
data_dir = "~/.config/aco"
log_level = "info"
theme = "dark"
auto_update = false
telemetry = false

[workflow]
max_plan_revisions = 3
max_repair_loops = 3
max_parallel_workers = 8
max_total_tokens = 5_000_000
max_wallclock_secs = 14400
max_user_query_wait = 3600

[ui]
show_token_stream = false
show_console = true
console_height_px = 240
timeline_position = "bottom"

[logging]
redact = ["*KEY*", "*TOKEN*"]
format = "json"
sample_console = 1.0
sample_events = 0.1

[storage]
retention_usage_days = 365
retention_prompts_days = 180
backup_dir = "~/.config/aco/backups"
backup_daily_keep = 7
backup_weekly_keep = 4

[security]
allow_unrestricted_plugins = false
confirm_external_writes = true
require_signature_for_plugins = true
"#;
        let cfg: AcoConfig = toml::from_str(toml).expect("parse");
        assert_eq!(cfg.workflow.max_parallel_workers, 8);
        assert_eq!(cfg.ui.timeline_position, TimelinePosition::Bottom);
    }
}
