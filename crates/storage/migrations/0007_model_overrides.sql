-- v0.4.30: per-(provider_id, model_id) metadata overrides.
--
-- Use case: built-in providers ship with a fallback model list
-- (see crates/pipe-server/src/providers.rs::OPENAI_FALLBACK_MODELS).
-- When the upstream vendor publishes a new context-window or
-- thinking-tier value, the chairman wants to fix the local entry
-- without waiting for an app release. This table stores the user's
-- override; the GET /api/providers handler overlays it on top of
-- the built-in list at read time.
--
-- Both columns are nullable so the chairman can set just one
-- (e.g. context_length only). `updated_at` is wall-clock seconds
-- since epoch for diagnostic logging only; not enforced unique.
--
-- Idempotent — existing DBs from v0.4.29 don't have this table
-- and the bin's migrate() loop runs every startup.

CREATE TABLE IF NOT EXISTS model_overrides (
    provider_id      TEXT    NOT NULL,
    model_id         TEXT    NOT NULL,
    context_length   INTEGER NULL,
    thinking_strength TEXT   NULL,
    updated_at       INTEGER NOT NULL,
    PRIMARY KEY (provider_id, model_id)
);