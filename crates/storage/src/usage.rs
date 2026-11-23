//! Quota-failure tracking.

use super::*;

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
                let r =
                    sqlx::query("DELETE FROM quota_failures WHERE role_id = ? AND model_id = ?")
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
}

impl Repository {
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
    async fn fetch_quota_rows(&self, sql: &str) -> Result<Vec<QuotaFailureRow>, StorageError> {
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
