-- 0003_secrets_and_providers.sql
-- Persistent storage for v0.4 user data:
--   * API key secrets (encrypted at rest via OS keystore)
--   * User-defined custom providers (relay stations / private gateways)
--   * Generic key/value table for "first_run", "kv.first_run_no_providers", etc.
--
-- The plaintext of an API key NEVER touches the SQLite database.
-- Only the AES-GCM ciphertext + nonce are stored here. The
-- encryption key lives in:
--   * Windows : DPAPI (via the `keyring` crate)
--   * macOS   : Keychain (via the `keyring` crate)
--   * Linux   : libsecret, or a passphrase-protected file under
--               $XDG_DATA_HOME/flowntier/master.key as fallback
--               when libsecret is unavailable (e.g. headless server,
--               minimal container).
--
-- See crates/pipe-server/src/secrets.rs for the encryption layer.
--
-- This migration is **not** destructive: existing databases from
-- v0.3 upgrade cleanly. Old plaintext `secrets.json` (if any)
-- migrates in app-code (see SecretsService::migrate_legacy_plaintext).

-- ── secret ────────────────────────────────────────────────────────
-- One row per stored secret. `name` is the env-var name we expose
-- the secret under (e.g. "OPENAI_API_KEY"); the value is what the
-- user pasted in the UI.
CREATE TABLE secret (
  name          TEXT PRIMARY KEY,
  ciphertext    BLOB NOT NULL,         -- AES-256-GCM ciphertext
  nonce         BLOB NOT NULL,         -- 12-byte nonce
  ad            BLOB NOT NULL DEFAULT X'', -- optional AAD (we store name)
  key_handle    TEXT NOT NULL,         -- id of the OS keystore entry that
                                        -- holds the data-encryption-key
  created_at    INTEGER NOT NULL,
  updated_at    INTEGER NOT NULL,
  last_used_at  INTEGER                -- null = never used
);

CREATE INDEX idx_secret_updated ON secret(updated_at DESC);

-- ── provider ──────────────────────────────────────────────────────
-- Built-in presets. Read from a Rust constant; this table is
-- here only so we can persist per-provider toggles and overrides.
-- One row per provider_id; the display_name, base_url, etc.
-- columns mirror the Rust preset table but can be overridden.
CREATE TABLE provider (
  id              TEXT PRIMARY KEY,    -- e.g. "openai", "anthropic", "google"
  enabled         INTEGER NOT NULL DEFAULT 1,  -- 0/1
  default_model   TEXT,                -- null = use preset default
  base_url        TEXT,                -- null = use preset default
  preset_json     TEXT NOT NULL,       -- full preset snapshot at first-write
  updated_at      INTEGER NOT NULL
);

-- ── custom_provider ───────────────────────────────────────────────
-- User-defined providers (relay stations, private gateways).
-- Has its own api_key like the presets, referenced by id.
CREATE TABLE custom_provider (
  id          TEXT PRIMARY KEY,        -- ulid
  name        TEXT NOT NULL,           -- human label
  base_url    TEXT NOT NULL,
  kind        TEXT NOT NULL,           -- "openai-compatible" | "anthropic-compatible"
  default_model TEXT,
  enabled     INTEGER NOT NULL DEFAULT 1,
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
);

-- ── model_cache ───────────────────────────────────────────────────
-- 1-hour cache of GET /api/providers/{id}/models responses.
-- Avoids hitting the provider's /v1/models endpoint every UI open.
CREATE TABLE model_cache (
  provider_id    TEXT PRIMARY KEY,    -- "openai" or custom ulid
  models_json    TEXT NOT NULL,       -- JSON array of {id, display_name}
  fetched_at     INTEGER NOT NULL     -- unix seconds
);

-- ── kv ────────────────────────────────────────────────────────────
-- Tiny key/value store for flags and short strings.
-- Keys must be ASCII; values are JSON-encoded.
CREATE TABLE kv (
  k     TEXT PRIMARY KEY,
  v     TEXT NOT NULL,                -- JSON-encoded
  mtime INTEGER NOT NULL
);

-- ── Pre-populate the provider table with all 9 presets ─────────────
-- This makes the (provider_id, has_secret) join trivial: LEFT JOIN
-- provider ON secret.name = 'PROVIDER_API_KEY' would not work
-- because secret names are env vars not provider ids. Instead,
-- secrets.rs::list_secrets() cross-references provider list +
-- name-pattern matching (e.g. provider "openai" → secret name
-- "OPENAI_API_KEY").
INSERT INTO provider (id, enabled, preset_json, updated_at) VALUES
  ('openai',      1, '{}', strftime('%s','now')),
  ('anthropic',   1, '{}', strftime('%s','now')),
  ('google',      1, '{}', strftime('%s','now')),
  ('deepseek',    1, '{}', strftime('%s','now')),
  ('minimax',     1, '{}', strftime('%s','now')),
  ('kimi',        1, '{}', strftime('%s','now')),
  ('glm',         1, '{}', strftime('%s','now')),
  ('mimo',        1, '{}', strftime('%s','now')),
  ('siliconflow', 1, '{}', strftime('%s','now'))
ON CONFLICT(id) DO NOTHING;

-- ── Set first_run flag ─────────────────────────────────────────────
INSERT INTO kv (k, v, mtime) VALUES
  ('first_run', 'true', strftime('%s','now'))
ON CONFLICT(k) DO NOTHING;