//! Tauri app glue: commands, menus, window management.
//!
//! This is the **only** crate that depends on `tauri`. All other
//! crates are library-only and unit-testable in isolation.
//!
//! See `docs/ARCHITECTURE.md` §3.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod logging;

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Top-level app state shared across Tauri commands.
#[derive(Debug, Clone)]
pub struct AppState {
    /// The event bus.
    pub bus: Arc<event_bus::EventBus>,
    /// The configuration.
    pub config: Arc<config::AcoConfig>,
    /// The storage repository.
    pub repo: Arc<storage::Repository>,
}

impl AppState {
    /// Build a fresh app state. Used by `main.rs` and tests.
    pub async fn build() -> Result<Self, BuildError> {
        // Defaults suitable for development; production reads
        // `flowntier.toml` from the OS-specific config dir.
        let data_dir = storage::Repository::default_data_dir()
            .ok_or_else(|| BuildError::NoDataDir)?;
        std::fs::create_dir_all(&data_dir)?;
        let db_path = data_dir.join("storage.sqlite");
        let repo = storage::Repository::open(&db_path).await?;
        let bus = event_bus::EventBus::default();
        let config = config::AcoConfig {
            app: config::AppSection {
                data_dir: data_dir.to_string_lossy().into_owned(),
                log_level: config::LogLevel::Info,
                theme: config::Theme::Dark,
                auto_update: false,
                telemetry: false,
            },
            workflow: config::WorkflowSection {
                max_plan_revisions: 3,
                max_repair_loops: 3,
                max_parallel_workers: 8,
                max_total_tokens: 5_000_000,
                max_wallclock_secs: 14_400,
                max_user_query_wait: 3_600,
            },
            ui: config::UiSection {
                show_token_stream: false,
                show_console: true,
                console_height_px: 240,
                timeline_position: config::TimelinePosition::Bottom,
            },
            logging: config::LoggingSection {
                redact: vec!["*KEY*".into(), "*TOKEN*".into()],
                format: config::LogFormat::Json,
                sample_console: 1.0,
                sample_events: 0.1,
            },
            storage: config::StorageSection {
                retention_usage_days: 365,
                retention_prompts_days: 180,
                backup_dir: data_dir.join("backups").to_string_lossy().into_owned(),
                backup_daily_keep: 7,
                backup_weekly_keep: 4,
            },
            security: config::SecuritySection {
                allow_unrestricted_plugins: false,
                confirm_external_writes: true,
                require_signature_for_plugins: true,
            },
        };

        Ok(Self {
            bus: Arc::new(bus),
            config: Arc::new(config),
            repo: Arc::new(repo),
        })
    }
}

/// Errors that can occur while building `AppState`.
#[derive(Debug, Error)]
pub enum BuildError {
    /// No OS-level data directory is available.
    #[error("no data directory available on this platform")]
    NoDataDir,
    /// Storage error.
    #[error("storage: {0}")]
    Storage(#[from] storage::StorageError),
    /// I/O error.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}



/// Tauri command payload: a new workflow request from the user.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NewWorkflowRequest {
    /// The user's free-form request.
    pub text: String,
    /// Optional project id (from `.aco/config.yaml`).
    pub project_id: Option<String>,
}

/// Tauri command response: a new workflow id.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NewWorkflowResponse {
    /// Workflow id.
    pub id: String,
}

/// Start a new workflow. Stub for Phase 0; real impl in Phase 1.
///
/// NOT annotated with `#[tauri::command]` because this is the core
/// library; the Tauri shell (`apps/desktop/src-tauri/src/lib.rs`)
/// wraps it in its own command. Adding `#[tauri::command]` here
/// would cause a duplicate-macro-definition error at link time.
pub async fn start_workflow(
    state: tauri::State<'_, AppState>,
    req: NewWorkflowRequest,
) -> Result<NewWorkflowResponse, String> {
    let id = ulid::Ulid::new().to_string();
    let now = chrono::Utc::now().timestamp();
    let wf = storage::Workflow {
        id: id.clone(),
        created_at: now,
        updated_at: now,
        state: "REQ_RECEIVED".into(),
        phase: "1-requirement".into(),
        user_request: req.text,
        plan_doc: None,
        summary: None,
        final_status: None,
        total_input_tokens: 0,
        total_output_tokens: 0,
        total_cost_usd: None,
    };
    state
        .repo
        .create_workflow(&wf)
        .await
        .map_err(|e| e.to_string())?;
    Ok(NewWorkflowResponse { id })
}
