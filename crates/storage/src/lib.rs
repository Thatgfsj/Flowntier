//! Storage layer: SQLite + FTS5 for ACO.
//!
//! All database access goes through this crate. No raw SQL elsewhere.
//!
//! See `docs/STORAGE_SPEC.md` for the full schema.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use thiserror::Error;

/// Storage errors.
#[derive(Debug, Error)]
pub enum StorageError {
    /// SQLx error.
    #[error("sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    /// Migration error.
    #[error("migration: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    /// I/O error.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Status of a workflow in the `workflows` table.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WorkflowStatus {
    /// Active workflow.
    Active,
    /// Successfully completed.
    Done,
    /// Failed unrecoverably.
    Failed,
    /// User/system aborted.
    Aborted,
}

impl WorkflowStatus {
    /// String form used in the database.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Done => "DONE",
            Self::Failed => "FAILED",
            Self::Aborted => "ABORTED",
        }
    }
}

/// A workflow row.
///
/// Note: derives `PartialEq` but not `Eq` because `total_cost_usd`
/// is `Option<f64>`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Workflow {
    /// Workflow id (ULID).
    pub id: String,
    /// Unix epoch seconds.
    pub created_at: i64,
    /// Unix epoch seconds.
    pub updated_at: i64,
    /// Current state.
    pub state: String,
    /// Current phase.
    pub phase: String,
    /// Original user request.
    pub user_request: String,
    /// Plan document (Markdown), if any.
    pub plan_doc: Option<String>,
    /// Final delivery summary.
    pub summary: Option<String>,
    /// Terminal status.
    pub final_status: Option<WorkflowStatus>,
    /// Total input tokens used.
    pub total_input_tokens: i64,
    /// Total output tokens used.
    pub total_output_tokens: i64,
    /// Total cost in USD.
    pub total_cost_usd: Option<f64>,
}

/// Repository wrapping a SQLite connection pool.
#[derive(Debug, Clone)]
pub struct Repository {
    pool: SqlitePool,
}

impl Repository {
    /// Open (or create) a SQLite database at the given path and run
    /// the embedded migrations.
    pub async fn open(path: &Path) -> Result<Self, StorageError> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let opts = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
            .busy_timeout(std::time::Duration::from_secs(5))
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(8)
            .connect_with(opts)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    /// Open an in-memory database (for tests).
    pub async fn open_in_memory() -> Result<Self, StorageError> {
        let opts = SqliteConnectOptions::new()
            .filename(":memory:")
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    /// Underlying pool. Use sparingly; prefer typed methods.
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Create a new workflow row.
    pub async fn create_workflow(&self, wf: &Workflow) -> Result<(), StorageError> {
        sqlx::query(
            r#"
            INSERT INTO workflows
              (id, created_at, updated_at, state, phase, user_request,
               plan_doc, summary, final_status, total_input_tokens,
               total_output_tokens, total_cost_usd)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&wf.id)
        .bind(wf.created_at)
        .bind(wf.updated_at)
        .bind(&wf.state)
        .bind(&wf.phase)
        .bind(&wf.user_request)
        .bind(&wf.plan_doc)
        .bind(&wf.summary)
        .bind(wf.final_status.map(|s| s.as_str().to_string()))
        .bind(wf.total_input_tokens)
        .bind(wf.total_output_tokens)
        .bind(wf.total_cost_usd)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Update the state and phase of a workflow.
    pub async fn update_workflow_state(
        &self,
        id: &str,
        state: &str,
        phase: &str,
    ) -> Result<(), StorageError> {
        let now = chrono::Utc::now().timestamp();
        sqlx::query("UPDATE workflows SET state = ?, phase = ?, updated_at = ? WHERE id = ?")
            .bind(state)
            .bind(phase)
            .bind(now)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Fetch a workflow by id.
    pub async fn get_workflow(&self, id: &str) -> Result<Option<Workflow>, StorageError> {
        let row: Option<WorkflowRow> =
            sqlx::query_as("SELECT * FROM workflows WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(Into::into))
    }

    /// Default location of the SQLite file.
    ///
    /// Honors the `aco` → `flowntier` rename (2026-06-24). On first
    /// call after upgrading, if a legacy `aco/` directory exists at
    /// the same parent and the new `flowntier/` directory does not
    /// yet, the legacy directory is renamed in place. The migration
    /// is best-effort and logged once; if the rename fails (perms,
    /// locked file, ...) we silently fall back to the new path.
    #[must_use]
    pub fn default_data_dir() -> Option<PathBuf> {
        let new = dirs::data_dir().map(|p| p.join("flowntier"));
        if let Some(dir) = &new {
            if !dir.exists() {
                if let Some(legacy) = dirs::data_dir().map(|p| p.join("aco")) {
                    if legacy.exists() {
                        match std::fs::rename(&legacy, dir) {
                            Ok(()) => eprintln!(
                                "[flowntier] migrated data dir: {} -> {}",
                                legacy.display(),
                                dir.display()
                            ),
                            Err(e) => eprintln!(
                                "[flowntier] could not migrate legacy data dir {} ({}); using fresh {}",
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
}

#[derive(sqlx::FromRow)]
struct WorkflowRow {
    id: String,
    created_at: i64,
    updated_at: i64,
    state: String,
    phase: String,
    user_request: String,
    plan_doc: Option<String>,
    summary: Option<String>,
    final_status: Option<String>,
    total_input_tokens: i64,
    total_output_tokens: i64,
    total_cost_usd: Option<f64>,
}

impl From<WorkflowRow> for Workflow {
    fn from(r: WorkflowRow) -> Self {
        Self {
            id: r.id,
            created_at: r.created_at,
            updated_at: r.updated_at,
            state: r.state,
            phase: r.phase,
            user_request: r.user_request,
            plan_doc: r.plan_doc,
            summary: r.summary,
            final_status: r
                .final_status
                .as_deref()
                .and_then(workflow_status_from_str),
            total_input_tokens: r.total_input_tokens,
            total_output_tokens: r.total_output_tokens,
            total_cost_usd: r.total_cost_usd,
        }
    }
}

fn workflow_status_from_str(s: &str) -> Option<WorkflowStatus> {
    match s {
        "ACTIVE" => Some(WorkflowStatus::Active),
        "DONE" => Some(WorkflowStatus::Done),
        "FAILED" => Some(WorkflowStatus::Failed),
        "ABORTED" => Some(WorkflowStatus::Aborted),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn create_and_fetch_workflow() {
        let repo = Repository::open_in_memory().await.expect("open");
        let now = chrono::Utc::now().timestamp();
        let wf = Workflow {
            id: "wf_01TEST".into(),
            created_at: now,
            updated_at: now,
            state: "REQ_RECEIVED".into(),
            phase: "1-requirement".into(),
            user_request: "Add /login".into(),
            plan_doc: None,
            summary: None,
            final_status: None,
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_cost_usd: None,
        };
        repo.create_workflow(&wf).await.expect("create");
        let fetched = repo.get_workflow("wf_01TEST").await.expect("fetch");
        assert_eq!(fetched, Some(wf));
    }
}
