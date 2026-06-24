-- 0002_rename_aco_to_flowntier.sql
-- Rename the `aco_toml` column on `config_snapshots` to reflect the
-- Flowntier rebrand. See plans/Rename-to-Flowntier.md.
--
-- This migration is idempotent at the column level: a DB that was
-- initialized after the rename shipped (no `aco_toml` column) will
-- report "duplicate column name" and abort. SQLite supports
-- `ALTER TABLE RENAME COLUMN` since 3.25.0; we ship that minimum
-- (Rust 1.85 / sqlx 0.8 targets ≥ 3.25 via the `bundled` feature).
--
-- If you need to roll forward across an in-flight DB that already
-- has `flowntier_toml` from a hand-edit, see the rollback note in
-- docs/RENAME_FLOWNTIER.md (TODO).

ALTER TABLE config_snapshots RENAME COLUMN aco_toml TO flowntier_toml;