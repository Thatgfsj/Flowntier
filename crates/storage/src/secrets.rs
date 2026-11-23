//! Secrets, built-in providers, and custom providers.

use super::*;

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
        let r: Option<(
            String,
            Vec<u8>,
            Vec<u8>,
            Vec<u8>,
            String,
            i64,
            i64,
            Option<i64>,
        )> = sqlx::query_as(
            "SELECT name, ciphertext, nonce, ad, key_handle, created_at, updated_at, last_used_at
                 FROM secret WHERE name = ?",
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;
        Ok(r.map(
            |(name, ciphertext, nonce, ad, key_handle, created_at, updated_at, last_used_at)| {
                SecretRow {
                    name,
                    ciphertext,
                    nonce,
                    ad,
                    key_handle,
                    created_at,
                    updated_at,
                    last_used_at,
                }
            },
        ))
    }

    /// List all secret names + metadata (NEVER returns ciphertext).
    pub async fn list_secret_meta(&self) -> Result<Vec<SecretRow>, StorageError> {
        let rows: Vec<(
            String,
            Vec<u8>,
            Vec<u8>,
            Vec<u8>,
            String,
            i64,
            i64,
            Option<i64>,
        )> = sqlx::query_as(
            "SELECT name, ciphertext, nonce, ad, key_handle, created_at, updated_at, last_used_at
                 FROM secret ORDER BY updated_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(
                    name,
                    ciphertext,
                    nonce,
                    ad,
                    key_handle,
                    created_at,
                    updated_at,
                    last_used_at,
                )| {
                    SecretRow {
                        name,
                        ciphertext,
                        nonce,
                        ad,
                        key_handle,
                        created_at,
                        updated_at,
                        last_used_at,
                    }
                },
            )
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
    pub async fn touch_secret_last_used(&self, name: &str) -> Result<bool, StorageError> {
        let res =
            sqlx::query("UPDATE secret SET last_used_at = strftime('%s','now') WHERE name = ?")
                .bind(name)
                .execute(&self.pool)
                .await?;
        Ok(res.rows_affected() > 0)
    }

    // ── Provider CRUD ───────────────────────────────────────────

    pub async fn get_provider(&self, id: &str) -> Result<Option<ProviderRow>, StorageError> {
        let r: Option<(String, i64, Option<String>, Option<String>, String, i64)> = sqlx::query_as(
            "SELECT id, enabled, default_model, base_url, preset_json, updated_at
                 FROM provider WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(r.map(
            |(id, enabled, default_model, base_url, preset_json, updated_at)| ProviderRow {
                id,
                enabled: enabled != 0,
                default_model,
                base_url,
                preset_json,
                updated_at,
            },
        ))
    }

    pub async fn list_providers(&self) -> Result<Vec<ProviderRow>, StorageError> {
        let rows: Vec<(String, i64, Option<String>, Option<String>, String, i64)> = sqlx::query_as(
            "SELECT id, enabled, default_model, base_url, preset_json, updated_at
                 FROM provider ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(id, enabled, default_model, base_url, preset_json, updated_at)| ProviderRow {
                    id,
                    enabled: enabled != 0,
                    default_model,
                    base_url,
                    preset_json,
                    updated_at,
                },
            )
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

    /// v0.4.34 (audit 000132, root fix): flip the `enabled` flag
    /// on a built-in provider row. Used by `resolve_role` when the
    /// keychain reveals an empty value, so the Settings UI shows
    /// the provider as off instead of looking "ready but broken".
    ///
    /// No-op when the row does not yet exist (the GET endpoint
    /// creates it lazily); we only UPDATE, never INSERT.
    pub async fn set_provider_enabled(
        &self,
        id: &str,
        enabled: bool,
    ) -> Result<bool, StorageError> {
        let n = sqlx::query(
            "UPDATE provider SET enabled = ?, updated_at = strftime('%s','now')
             WHERE id = ?",
        )
        .bind(enabled as i64)
        .bind(id)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(n > 0)
    }

    /// v0.4.34 (audit 000132, root fix): scan every
    /// `role_overrides` row and rewrite any whose `default_model`
    /// or any element of `fallback_chain` starts with
    /// `<provider_short>:`.
    ///
    /// Returns the number of rows touched. Used when a provider's
    /// API key disappears so the orchestrator stops retrying with
    /// a stale role assignment; the next workflow run then lands
    /// on the existing "role not configured" branch in
    /// `resolve_role` (handlers.rs:2464) which is the only error
    /// message that points the user at a fixable action.
    ///
    /// Only relevant for built-in presets — custom providers
    /// resolve their secret via a different code path and the
    /// caller (handlers.rs) decides whether to invoke this.
    pub async fn clear_role_overrides_for_provider(
        &self,
        provider_short: &str,
    ) -> Result<usize, StorageError> {
        let prefix = format!("{provider_short}:");

        // Snapshot every row (there are at most a dozen roles —
        // chief / critic:a / critic:b / worker / ...). Doing the
        // filter in Rust keeps the JSON parse + chain rewrite
        // obvious; rewriting raw SQL for the filter would obscure
        // the cascade semantics behind JSON1 expressions.
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT role_id, default_model, fallback_chain
             FROM role_overrides",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut touched = 0usize;
        for (role_id, default_model, fallback_chain_json) in rows {
            let mut changed = false;
            let new_default = if default_model.starts_with(&prefix) {
                changed = true;
                String::new()
            } else {
                default_model
            };

            let chain: Vec<String> = serde_json::from_str(&fallback_chain_json).unwrap_or_default();
            let new_chain: Vec<String> = chain
                .into_iter()
                .filter(|s| !s.starts_with(&prefix))
                .collect();
            if new_chain.len()
                != serde_json::from_str::<Vec<String>>(&fallback_chain_json)
                    .map(|v| v.len())
                    .unwrap_or(0)
            {
                changed = true;
            }

            if !changed {
                continue;
            }
            let new_chain_json = serde_json::to_string(&new_chain).unwrap_or_else(|_| "[]".into());
            sqlx::query(
                "UPDATE role_overrides
                 SET default_model = ?, fallback_chain = ?, updated_at = strftime('%s','now')
                 WHERE role_id = ?",
            )
            .bind(&new_default)
            .bind(&new_chain_json)
            .bind(&role_id)
            .execute(&self.pool)
            .await?;
            touched += 1;
        }
        Ok(touched)
    }

    // ── Custom provider CRUD ────────────────────────────────────

    pub async fn list_custom_providers(&self) -> Result<Vec<CustomProvider>, StorageError> {
        let rows: Vec<(
            String,
            String,
            String,
            String,
            Option<String>,
            i64,
            i64,
            i64,
        )> = sqlx::query_as(
            "SELECT id, name, base_url, kind, default_model, enabled, created_at, updated_at
                 FROM custom_provider ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(id, name, base_url, kind, default_model, enabled, created_at, updated_at)| {
                    CustomProvider {
                        id,
                        name,
                        base_url,
                        kind,
                        default_model,
                        enabled: enabled != 0,
                        created_at,
                        updated_at,
                    }
                },
            )
            .collect())
    }

    pub async fn get_custom_provider(
        &self,
        id: &str,
    ) -> Result<Option<CustomProvider>, StorageError> {
        let r: Option<(
            String,
            String,
            String,
            String,
            Option<String>,
            i64,
            i64,
            i64,
        )> = sqlx::query_as(
            "SELECT id, name, base_url, kind, default_model, enabled, created_at, updated_at
                 FROM custom_provider WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(r.map(
            |(id, name, base_url, kind, default_model, enabled, created_at, updated_at)| {
                CustomProvider {
                    id,
                    name,
                    base_url,
                    kind,
                    default_model,
                    enabled: enabled != 0,
                    created_at,
                    updated_at,
                }
            },
        ))
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
}
