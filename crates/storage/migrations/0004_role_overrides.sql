-- 0004_role_overrides.sql
-- v0.4.18 (event 000054): persist role -> model assignments so the
-- chairman's selections survive a runtime / Tauri restart. Previously
-- PUT /api/router/roles was a no-op stub that returned ok:true but
-- did not persist; on next refresh, the in-memory defaults won back
-- and the chairman's selection was gone.
--
-- One row per role id (e.g. "agent:chief"). default_model and
-- fallback_chain override the in-memory defaults returned by GET
-- /api/router/roles.

CREATE TABLE IF NOT EXISTS role_overrides (
    role_id        TEXT PRIMARY KEY NOT NULL,
    default_model  TEXT NOT NULL DEFAULT '',
    -- JSON array of "provider:model" strings. Empty array = no
    -- fallback. We store as TEXT (not a join table) so the schema
    -- stays small and the array order is preserved.
    fallback_chain TEXT NOT NULL DEFAULT '[]',
    updated_at     INTEGER NOT NULL
);