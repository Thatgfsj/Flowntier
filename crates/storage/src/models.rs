//! Model cache, model overrides, disabled models, role overrides.

use super::*;

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

impl Repository {
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

    /// v0.4.30 (audit 000130): drop the cached model list so the
    /// next GET re-applies model_overrides on top of the
    /// built-in fallback. Used by PUT/DELETE
    /// `/api/providers/{id}/models/{model}`.
    pub async fn delete_model_cache(&self, provider_id: &str) -> Result<bool, StorageError> {
        let r = sqlx::query("DELETE FROM model_cache WHERE provider_id = ?")
            .bind(provider_id)
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }

    pub async fn get_model_cache(
        &self,
        provider_id: &str,
    ) -> Result<Option<ModelCacheRow>, StorageError> {
        let r: Option<(String, String, i64)> = sqlx::query_as(
            "SELECT provider_id, models_json, fetched_at FROM model_cache WHERE provider_id = ?",
        )
        .bind(provider_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(
            r.map(|(provider_id, models_json, fetched_at)| ModelCacheRow {
                provider_id,
                models_json,
                fetched_at,
            }),
        )
    }

    // ── v0.4.30: per-model metadata overrides (audit 000130) ──
    //
    // Built-in providers ship with a fallback model list
    // (see crates/pipe-server/src/providers.rs::OPENAI_FALLBACK_MODELS).
    // When the upstream vendor publishes a new context-window or
    // thinking-tier value, the chairman wants to fix the local entry
    // without waiting for an app release. The GET /api/providers
    // handler overlays this table on top of the built-in list at
    // read time; the user clears it with the DELETE endpoint.

    /// Insert or replace the metadata override for a single
    /// `(provider_id, model_id)` pair. `context_length` and
    /// `thinking_strength` are both optional — pass `None` to keep
    /// the existing value, or `Some(None)` to clear it. The handler
    /// layer is responsible for serialising the optional-optional
    /// double-Option shape; this method just upserts.
    pub async fn upsert_model_override(
        &self,
        provider_id: &str,
        model_id: &str,
        context_length: Option<Option<i64>>,
        thinking_strength: Option<Option<String>>,
    ) -> Result<(), StorageError> {
        // Read existing row so we can merge partial updates without
        // nuking columns the caller didn't pass.
        let existing: Option<(Option<i64>, Option<String>)> = sqlx::query_as(
            "SELECT context_length, thinking_strength
               FROM model_overrides
              WHERE provider_id = ? AND model_id = ?",
        )
        .bind(provider_id)
        .bind(model_id)
        .fetch_optional(&self.pool)
        .await?;
        let (existing_ctx, existing_ts) = match existing {
            Some((c, t)) => (c, t),
            None => (None, None),
        };
        let new_ctx = context_length.unwrap_or(existing_ctx);
        let new_ts = thinking_strength.unwrap_or(existing_ts);

        sqlx::query(
            "INSERT INTO model_overrides
                 (provider_id, model_id, context_length, thinking_strength, updated_at)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(provider_id, model_id) DO UPDATE SET
                 context_length = excluded.context_length,
                 thinking_strength = excluded.thinking_strength,
                 updated_at = excluded.updated_at",
        )
        .bind(provider_id)
        .bind(model_id)
        .bind(new_ctx)
        .bind(new_ts)
        .bind(chrono::Utc::now().timestamp())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Delete the metadata override for a `(provider_id, model_id)`
    /// pair. Returns `true` if a row was removed, `false` if the
    /// pair had no override (the built-in fallback then wins).
    pub async fn delete_model_override(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<bool, StorageError> {
        let r = sqlx::query("DELETE FROM model_overrides WHERE provider_id = ? AND model_id = ?")
            .bind(provider_id)
            .bind(model_id)
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }

    /// Bulk-read all overrides for a single provider. Used by
    /// GET /api/providers to overlay on top of the built-in list.
    pub async fn list_model_overrides_for_provider(
        &self,
        provider_id: &str,
    ) -> Result<Vec<(String, Option<i64>, Option<String>)>, StorageError> {
        let rows: Vec<(String, Option<i64>, Option<String>)> = sqlx::query_as(
            "SELECT model_id, context_length, thinking_strength
               FROM model_overrides
              WHERE provider_id = ?",
        )
        .bind(provider_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    // ── Disabled models (event 000110 fix D1) ────────────────
    //
    // The user can hide a model from the provider's catalog (e.g.
    // an expired tier). Persisted as (provider_id, model_id) pairs;
    // absence = enabled, presence = disabled. The list_*/filter
    // helpers in handlers.rs consult `list_disabled_models` so the
    // disabled models disappear from the Settings UI AND from
    // list_router_models (so the role router cannot pick them).

    /// Record that `(provider_id, model_id)` should be hidden from
    /// the catalog. Idempotent: re-inserting the same pair is a
    /// no-op (the PRIMARY KEY guarantees uniqueness).
    pub async fn disable_model(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO disabled_models (provider_id, model_id, disabled_at)
             VALUES (?, ?, ?)
             ON CONFLICT(provider_id, model_id) DO NOTHING",
        )
        .bind(provider_id)
        .bind(model_id)
        .bind(chrono::Utc::now().timestamp())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Un-hide a previously disabled `(provider_id, model_id)`.
    /// Returns `true` if a row was actually deleted, `false` if the
    /// pair was not in the table (i.e. the user re-enabled a model
    /// that was never disabled — harmless no-op).
    pub async fn enable_model(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<bool, StorageError> {
        let r = sqlx::query("DELETE FROM disabled_models WHERE provider_id = ? AND model_id = ?")
            .bind(provider_id)
            .bind(model_id)
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }

    /// Return every `(provider_id, model_id)` pair the user has
    /// hidden. Used by `list_models` / `list_router_models` /
    /// `list_providers` to filter catalogs before sending them to
    /// the webview.
    pub async fn list_disabled_models(&self) -> Result<Vec<(String, String)>, StorageError> {
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT provider_id, model_id FROM disabled_models")
                .fetch_all(&self.pool)
                .await?;
        Ok(rows)
    }

    /// Convenience: is this single `(provider_id, model_id)` in the
    /// disabled set? Most callers want the bulk list and cache the
    /// HashSet; this exists for the toggle handler so it can return
    /// a precise 404 if the user re-enables a non-disabled row.
    pub async fn is_model_disabled(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<bool, StorageError> {
        let r: Option<(i64,)> =
            sqlx::query_as("SELECT 1 FROM disabled_models WHERE provider_id = ? AND model_id = ?")
                .bind(provider_id)
                .bind(model_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(r.is_some())
    }
}

impl Repository {
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
                let chain: Vec<String> = serde_json::from_str(&fallback_chain).unwrap_or_default();
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
}
