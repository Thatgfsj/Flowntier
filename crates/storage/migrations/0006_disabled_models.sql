-- 0006_disabled_models.sql
-- event 000110 (fix D1): per-(provider, model) disable list.
-- When the user hides a model from the provider catalog (because
-- it expired, they no longer have access, etc.), we record it here
-- so list_models / list_router_models / list_providers can filter it
-- out of role-router options and the Settings UI.
--
-- One row per (provider_id, model_id). Toggling is an explicit
-- insert/delete — there is no `enabled` column because the absence
-- of a row means "enabled" and presence means "disabled".
--
-- This migration is purely additive; existing databases upgrade
-- cleanly (CREATE TABLE IF NOT EXISTS).

CREATE TABLE IF NOT EXISTS disabled_models (
    provider_id    TEXT    NOT NULL,
    model_id       TEXT    NOT NULL,
    disabled_at    INTEGER NOT NULL,
    PRIMARY KEY (provider_id, model_id)
);

-- The list_disabled_models() query scans by provider_id; keep that
-- index so the per-provider filter stays O(log n) instead of O(n).
CREATE INDEX IF NOT EXISTS idx_disabled_models_provider
    ON disabled_models (provider_id);