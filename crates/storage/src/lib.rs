//! Storage layer: SQLite + FTS5 for Flowntier.
//!
//! All database access goes through this crate. No raw SQL elsewhere.
//!
//! See `docs/STORAGE_SPEC.md` for the full schema.

#![forbid(unsafe_code)]
#![allow(missing_docs)]  // internal crate: API documented in STORAGE_SPEC.md and module-level //! docs; per-field doc comments would duplicate the type signature.

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

// ── v0.4 secret / provider types ─────────────────────────────────
// See migrations/0003_secrets_and_providers.sql.

/// A stored secret. The plaintext lives in the OS keystore; this
/// row only holds the AES-GCM ciphertext + a key handle pointing
/// to the master encryption key in the keystore.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretRow {
    pub name: String,
    pub ciphertext: Vec<u8>,
    pub nonce: Vec<u8>,
    pub ad: Vec<u8>,
    pub key_handle: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_used_at: Option<i64>,
}

/// Provider preset (one of the 9 built-in).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRow {
    pub id: String,
    pub enabled: bool,
    pub default_model: Option<String>,
    pub base_url: Option<String>,
    pub preset_json: String,
    pub updated_at: i64,
}

/// User-defined provider (relay station / private gateway).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomProvider {
    pub id: String,
    pub name: String,
    pub base_url: String,
    /// "openai-compatible" | "anthropic-compatible"
    pub kind: String,
    pub default_model: Option<String>,
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Cached GET /api/providers/{id}/models response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCacheRow {
    pub provider_id: String,
    pub models_json: String,
    pub fetched_at: i64,
}

// ── v0.4.18 role overrides ───────────────────────────────────────
// See migrations/0004_role_overrides.sql.

/// Per-role override of `default_model` and `fallback_chain`. One
/// row per role id (e.g. `"agent:chief"`). An empty row
/// (default_model="", fallback_chain="[]") is a valid override
/// meaning "user explicitly cleared the in-memory defaults".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleOverrideRow {
    pub role_id: String,
    pub default_model: String,
    /// Stored as a JSON-encoded array string in the DB; we keep
    /// it as String here to avoid forcing the storage layer to
    /// know about the agent-core role schema.
    pub fallback_chain: Vec<String>,
    pub updated_at: i64,
}

/// v0.4.20 (event 000056): one row per (role, model) quota
/// failure. Lives in `quota_failures` (migration 0005). See
/// `Repository::record_quota_failure` for the state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaFailureRow {
    pub role_id: String,
    pub model_id: String,
    pub last_error_at: i64,
    pub last_error_message: String,
    /// One of: "failed" | "pending_5h_wait" | "rate_limited".
    /// We use a String (not an enum) so a future schema addition
    /// doesn't force a storage-layer release; the handler layer
    /// narrows it before serialising.
    pub status: String,
    pub attempt_count: i64,
    pub next_attempt_at: Option<i64>,
}

impl Repository {
    // ── Secret CRUD ────────────────────────────────────────────

    /// Insert or replace a secret row.
    pub async fn put_secret(&self, s: &SecretRow) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO secret
                (name, ciphertext, nonce, ad, key_handle, created_at, updated_at, last_used_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(name) DO UPDATE SET
                ciphertext=excluded.ciphertext,
                nonce=excluded.nonce,
                ad=excluded.ad,
                key_handle=excluded.key_handle,
                updated_at=excluded.updated_at",
        )
        .bind(&s.name)
        .bind(&s.ciphertext)
        .bind(&s.nonce)
        .bind(&s.ad)
        .bind(&s.key_handle)
        .bind(s.created_at)
        .bind(s.updated_at)
        .bind(s.last_used_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Fetch a single secret row. Returns None if not found.
    pub async fn get_secret(&self, name: &str) -> Result<Option<SecretRow>, StorageError> {
        let r: Option<(String, Vec<u8>, Vec<u8>, Vec<u8>, String, i64, i64, Option<i64>)> =
            sqlx::query_as(
                "SELECT name, ciphertext, nonce, ad, key_handle, created_at, updated_at, last_used_at
                 FROM secret WHERE name = ?",
            )
            .bind(name)
            .fetch_optional(&self.pool)
            .await?;
        Ok(r.map(|(name, ciphertext, nonce, ad, key_handle, created_at, updated_at, last_used_at)| {
            SecretRow { name, ciphertext, nonce, ad, key_handle, created_at, updated_at, last_used_at }
        }))
    }

    /// List all secret names + metadata (NEVER returns ciphertext).
    pub async fn list_secret_meta(&self) -> Result<Vec<SecretRow>, StorageError> {
        let rows: Vec<(String, Vec<u8>, Vec<u8>, Vec<u8>, String, i64, i64, Option<i64>)> =
            sqlx::query_as(
                "SELECT name, ciphertext, nonce, ad, key_handle, created_at, updated_at, last_used_at
                 FROM secret ORDER BY updated_at DESC",
            )
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter()
            .map(|(name, ciphertext, nonce, ad, key_handle, created_at, updated_at, last_used_at)| {
                SecretRow { name, ciphertext, nonce, ad, key_handle, created_at, updated_at, last_used_at }
            })
            .collect())
    }

    /// Delete a secret row. The matching keystore entry is the
    /// caller's responsibility (see secrets.rs::delete()).
    pub async fn delete_secret(&self, name: &str) -> Result<bool, StorageError> {
        let res = sqlx::query("DELETE FROM secret WHERE name = ?")
            .bind(name)
            .execute(&self.pool)
            .await?;
        Ok(res.rows_affected() > 0)
    }

    /// Update only the `last_used_at` column. Used by the secret
    /// store as an audit trail when reveal() is called.
    pub async fn touch_secret_last_used(
        &self,
        name: &str,
    ) -> Result<bool, StorageError> {
        let res = sqlx::query(
            "UPDATE secret SET last_used_at = strftime('%s','now') WHERE name = ?",
        )
        .bind(name)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    // ── Provider CRUD ───────────────────────────────────────────

    pub async fn get_provider(&self, id: &str) -> Result<Option<ProviderRow>, StorageError> {
        let r: Option<(String, i64, Option<String>, Option<String>, String, i64)> =
            sqlx::query_as(
                "SELECT id, enabled, default_model, base_url, preset_json, updated_at
                 FROM provider WHERE id = ?",
            )
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(r.map(|(id, enabled, default_model, base_url, preset_json, updated_at)| ProviderRow {
            id, enabled: enabled != 0, default_model, base_url, preset_json, updated_at,
        }))
    }

    pub async fn list_providers(&self) -> Result<Vec<ProviderRow>, StorageError> {
        let rows: Vec<(String, i64, Option<String>, Option<String>, String, i64)> =
            sqlx::query_as(
                "SELECT id, enabled, default_model, base_url, preset_json, updated_at
                 FROM provider ORDER BY id",
            )
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter()
            .map(|(id, enabled, default_model, base_url, preset_json, updated_at)| ProviderRow {
                id, enabled: enabled != 0, default_model, base_url, preset_json, updated_at,
            })
            .collect())
    }

    pub async fn upsert_provider(&self, p: &ProviderRow) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO provider (id, enabled, default_model, base_url, preset_json, updated_at)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                enabled=excluded.enabled,
                default_model=excluded.default_model,
                base_url=excluded.base_url,
                preset_json=excluded.preset_json,
                updated_at=excluded.updated_at",
        )
        .bind(&p.id)
        .bind(p.enabled as i64)
        .bind(&p.default_model)
        .bind(&p.base_url)
        .bind(&p.preset_json)
        .bind(p.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // ── Custom provider CRUD ────────────────────────────────────

    pub async fn list_custom_providers(&self) -> Result<Vec<CustomProvider>, StorageError> {
        let rows: Vec<(String, String, String, String, Option<String>, i64, i64, i64)> =
            sqlx::query_as(
                "SELECT id, name, base_url, kind, default_model, enabled, created_at, updated_at
                 FROM custom_provider ORDER BY created_at DESC",
            )
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter()
            .map(|(id, name, base_url, kind, default_model, enabled, created_at, updated_at)| {
                CustomProvider {
                    id, name, base_url, kind, default_model,
                    enabled: enabled != 0, created_at, updated_at,
                }
            })
            .collect())
    }

    pub async fn get_custom_provider(&self, id: &str) -> Result<Option<CustomProvider>, StorageError> {
        let r: Option<(String, String, String, String, Option<String>, i64, i64, i64)> =
            sqlx::query_as(
                "SELECT id, name, base_url, kind, default_model, enabled, created_at, updated_at
                 FROM custom_provider WHERE id = ?",
            )
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(r.map(|(id, name, base_url, kind, default_model, enabled, created_at, updated_at)| {
            CustomProvider {
                id, name, base_url, kind, default_model,
                enabled: enabled != 0, created_at, updated_at,
            }
        }))
    }

    pub async fn insert_custom_provider(&self, p: &CustomProvider) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO custom_provider
                (id, name, base_url, kind, default_model, enabled, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&p.id)
        .bind(&p.name)
        .bind(&p.base_url)
        .bind(&p.kind)
        .bind(&p.default_model)
        .bind(p.enabled as i64)
        .bind(p.created_at)
        .bind(p.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_custom_provider(&self, id: &str) -> Result<bool, StorageError> {
        let res = sqlx::query("DELETE FROM custom_provider WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(res.rows_affected() > 0)
    }

    // ── Model cache ──────────────────────────────────────────────

    pub async fn put_model_cache(&self, c: &ModelCacheRow) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO model_cache (provider_id, models_json, fetched_at)
             VALUES (?, ?, ?)
             ON CONFLICT(provider_id) DO UPDATE SET
                models_json=excluded.models_json,
                fetched_at=excluded.fetched_at",
        )
        .bind(&c.provider_id)
        .bind(&c.models_json)
        .bind(c.fetched_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_model_cache(&self, provider_id: &str) -> Result<Option<ModelCacheRow>, StorageError> {
        let r: Option<(String, String, i64)> = sqlx::query_as(
            "SELECT provider_id, models_json, fetched_at FROM model_cache WHERE provider_id = ?",
        )
        .bind(provider_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(r.map(|(provider_id, models_json, fetched_at)| ModelCacheRow {
            provider_id, models_json, fetched_at,
        }))
    }

    // ── Generic kv ──────────────────────────────────────────────

    pub async fn kv_get(&self, k: &str) -> Result<Option<String>, StorageError> {
        let r: Option<(String,)> = sqlx::query_as("SELECT v FROM kv WHERE k = ?")
            .bind(k)
            .fetch_optional(&self.pool)
            .await?;
        Ok(r.map(|(v,)| v))
    }

    pub async fn kv_set(&self, k: &str, v: &str) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO kv (k, v, mtime) VALUES (?, ?, strftime('%s','now'))
             ON CONFLICT(k) DO UPDATE SET v=excluded.v, mtime=strftime('%s','now')",
        )
        .bind(k)
        .bind(v)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // ── v0.4.18 role overrides (migrations/0004_role_overrides.sql) ──

    /// Read a single role override. Returns `None` if the role has
    /// never been set explicitly (the in-memory default applies).
    pub async fn get_role_override(
        &self,
        role_id: &str,
    ) -> Result<Option<RoleOverrideRow>, StorageError> {
        let row: Option<(String, String, String, i64)> = sqlx::query_as(
            "SELECT role_id, default_model, fallback_chain, updated_at
             FROM role_overrides WHERE role_id = ?",
        )
        .bind(role_id)
        .fetch_optional(&self.pool)
        .await?;
        match row {
            None => Ok(None),
            Some((role_id, default_model, fallback_chain, updated_at)) => {
                let chain: Vec<String> = serde_json::from_str(&fallback_chain)
                    .unwrap_or_default();
                Ok(Some(RoleOverrideRow {
                    role_id,
                    default_model,
                    fallback_chain: chain,
                    updated_at,
                }))
            }
        }
    }

    /// Upsert a role override. Caller passes the canonical list
    /// (already deduped); we re-serialize to JSON.
    pub async fn upsert_role_override(
        &self,
        role_id: &str,
        default_model: &str,
        fallback_chain: &[String],
    ) -> Result<(), StorageError> {
        let chain_json = serde_json::to_string(fallback_chain).unwrap_or_else(|_| "[]".into());
        sqlx::query(
            "INSERT INTO role_overrides (role_id, default_model, fallback_chain, updated_at)
             VALUES (?, ?, ?, strftime('%s','now'))
             ON CONFLICT(role_id) DO UPDATE SET
                default_model  = excluded.default_model,
                fallback_chain = excluded.fallback_chain,
                updated_at     = strftime('%s','now')",
        )
        .bind(role_id)
        .bind(default_model)
        .bind(chain_json)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Delete a role override (fall back to in-memory defaults).
    pub async fn delete_role_override(&self, role_id: &str) -> Result<bool, StorageError> {
        let n = sqlx::query("DELETE FROM role_overrides WHERE role_id = ?")
            .bind(role_id)
            .execute(&self.pool)
            .await?
            .rows_affected();
        Ok(n > 0)
    }

    /// List every role override (used by GET /api/router/roles to
    /// overlay the in-memory defaults).
    pub async fn list_role_overrides(&self) -> Result<Vec<RoleOverrideRow>, StorageError> {
        let rows: Vec<(String, String, String, i64)> = sqlx::query_as(
            "SELECT role_id, default_model, fallback_chain, updated_at
             FROM role_overrides",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut out = Vec::with_capacity(rows.len());
        for (role_id, default_model, fallback_chain, updated_at) in rows {
            let chain: Vec<String> = serde_json::from_str(&fallback_chain).unwrap_or_default();
            out.push(RoleOverrideRow {
                role_id,
                default_model,
                fallback_chain: chain,
                updated_at,
            });
        }
        Ok(out)
    }

    // ── v0.4.20 quota-failure tracker (migrations/0005_quota_failures.sql)
    // Each (role_id, model_id) row records the most recent failure
    // and the per-row state machine:
    //   failed          — any non-DONE run_task, awaiting escalation
    //   pending_5h_wait — chief's own (chief, model) row, awaiting the
    //                     next 5-hour boundary scheduler tick
    //   rate_limited    — the 5h-tick retry also failed; no further
    //                     auto-retries until the chairman clicks 重置
    //                     in Settings → 角色额度状态
    //
    // On any successful run_task against (role, model) we call
    // `clear_quota_failure(role, Some(model_id))` to DELETE the row,
    // so "auto-cleanup on recovery" is just an UPSERT-then-DELETE.

    /// Record or update a quota failure. UPSERT semantics:
    ///   - row absent → INSERT with attempt_count=1, status='failed'
    ///   - row present → UPDATE last_error_at, last_error_message,
    ///                   attempt_count=attempt_count+1; status stays
    ///                   whatever it was (we don't downgrade
    ///                   pending_5h_wait or rate_limited on a new
    ///                   failure because the scheduler owns those).
    pub async fn record_quota_failure(
        &self,
        role_id: &str,
        model_id: &str,
        error_message: &str,
    ) -> Result<(), StorageError> {
        let now = chrono::Utc::now().timestamp();
        sqlx::query(
            "INSERT INTO quota_failures
                (role_id, model_id, last_error_at, last_error_message, status, attempt_count)
             VALUES (?, ?, ?, ?, 'failed', 1)
             ON CONFLICT(role_id, model_id) DO UPDATE SET
                last_error_at = excluded.last_error_at,
                last_error_message = excluded.last_error_message,
                attempt_count = quota_failures.attempt_count + 1,
                -- If the row was already pending_5h_wait or
                -- rate_limited, preserve it; only newly-created or
                -- 'failed' rows reflect the new status here.
                status = CASE
                    WHEN quota_failures.status IN ('pending_5h_wait', 'rate_limited')
                        THEN quota_failures.status
                    ELSE 'failed'
                END",
        )
        .bind(role_id)
        .bind(model_id)
        .bind(now)
        .bind(error_message)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Promote (role, model) to `pending_5h_wait`. Called by the
    /// run_task handler when chief itself fails — chief's failure
    /// is the escalation boundary per the chairman's spec ("主理
    /// 也额度挂了 → 等最近5小时刷新点试一次").
    pub async fn set_quota_pending_5h_wait(
        &self,
        role_id: &str,
        model_id: &str,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE quota_failures
             SET status = 'pending_5h_wait', next_attempt_at = NULL
             WHERE role_id = ? AND model_id = ?",
        )
        .bind(role_id)
        .bind(model_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Mark (role, model) as `rate_limited`. Called by the
    /// scheduler after a 5h-tick retry also failed. No further
    /// auto-retries; only `clear_quota_failure` or `reset` will
    /// lift this.
    pub async fn mark_quota_rate_limited(
        &self,
        role_id: &str,
        model_id: &str,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE quota_failures
             SET status = 'rate_limited', next_attempt_at = NULL
             WHERE role_id = ? AND model_id = ?",
        )
        .bind(role_id)
        .bind(model_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Remove a (role, model) row. Called on success and from
    /// the chairman's "重置" button. `model_id == None` clears
    /// every row for that role (chief's "forget history" path).
    /// Returns the number of rows deleted.
    pub async fn clear_quota_failure(
        &self,
        role_id: &str,
        model_id: Option<&str>,
    ) -> Result<usize, StorageError> {
        let n = match model_id {
            Some(m) => {
                let r = sqlx::query(
                    "DELETE FROM quota_failures WHERE role_id = ? AND model_id = ?",
                )
                .bind(role_id)
                .bind(m)
                .execute(&self.pool)
                .await?;
                r.rows_affected() as usize
            }
            None => {
                let r = sqlx::query("DELETE FROM quota_failures WHERE role_id = ?")
                    .bind(role_id)
                    .execute(&self.pool)
                    .await?;
                r.rows_affected() as usize
            }
        };
        Ok(n)
    }

    /// List every row in `pending_5h_wait`. The scheduler calls
    /// this once per 5h tick. Backed by the partial index
    /// `idx_quota_pending` (WHERE status = 'pending_5h_wait').
    pub async fn list_pending_5h_wait(&self) -> Result<Vec<QuotaFailureRow>, StorageError> {
        self.fetch_quota_rows(
            "SELECT role_id, model_id, last_error_at, last_error_message, status, attempt_count, next_attempt_at
             FROM quota_failures WHERE status = 'pending_5h_wait'",
        )
        .await
    }

    /// List every quota failure row across all roles. Used by
    /// GET /api/quota/status (Settings → 角色额度状态).
    pub async fn list_all_quota_failures(&self) -> Result<Vec<QuotaFailureRow>, StorageError> {
        self.fetch_quota_rows(
            "SELECT role_id, model_id, last_error_at, last_error_message, status, attempt_count, next_attempt_at
             FROM quota_failures ORDER BY role_id, model_id",
        )
        .await
    }

    /// Read a single (role, model) row. Returns None if absent.
    /// Used by `resolve_role` to embed `quota_status` in the
    /// status-line response so the Settings UI can show
    /// "上次失败 · 等 5h 刷新点" inline next to the model select.
    pub async fn quota_status_for(
        &self,
        role_id: &str,
        model_id: &str,
    ) -> Result<Option<QuotaFailureRow>, StorageError> {
        let row: Option<(String, String, i64, String, String, i64, Option<i64>)> = sqlx::query_as(
            "SELECT role_id, model_id, last_error_at, last_error_message, status, attempt_count, next_attempt_at
             FROM quota_failures WHERE role_id = ? AND model_id = ?",
        )
        .bind(role_id)
        .bind(model_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(r, m, t, e, s, a, na)| QuotaFailureRow {
            role_id: r,
            model_id: m,
            last_error_at: t,
            last_error_message: e,
            status: s,
            attempt_count: a,
            next_attempt_at: na,
        }))
    }

    /// Internal helper: run a query that returns the 7-column
    /// shape used by every quota row reader.
    async fn fetch_quota_rows(
        &self,
        sql: &str,
    ) -> Result<Vec<QuotaFailureRow>, StorageError> {
        let rows: Vec<(String, String, i64, String, String, i64, Option<i64>)> =
            sqlx::query_as(sql).fetch_all(&self.pool).await?;
        let out = rows
            .into_iter()
            .map(|(r, m, t, e, s, a, na)| QuotaFailureRow {
                role_id: r,
                model_id: m,
                last_error_at: t,
                last_error_message: e,
                status: s,
                attempt_count: a,
                next_attempt_at: na,
            })
            .collect();
        Ok(out)
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
