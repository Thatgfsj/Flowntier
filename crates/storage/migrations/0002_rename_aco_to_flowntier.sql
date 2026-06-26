-- 0002_rename_aco_to_flowntier.sql
-- Rename the `aco_toml` column on `config_snapshots` to reflect the
-- Flowntier rebrand (see plans/Rename-to-Flowntier.md for the
-- overall plan).
--
-- This migration is idempotent at the column level: a DB that was
-- initialized after the rename shipped (no `aco_toml` column) will
-- report "duplicate column name" and abort. SQLite supports
-- `ALTER TABLE RENAME COLUMN` since 3.25.0; we ship that minimum
-- (Rust 1.85 / sqlx 0.8 targets ≥ 3.25 via the `bundled` feature).
--
-- Rollback: if you need to roll back from `flowntier_toml` to
-- `aco_toml` (e.g. downgrading v0.4 to v0.3), run:
--   ALTER TABLE config_snapshots RENAME COLUMN flowntier_toml TO aco_toml;
-- manually. There is no DOWN-migration here on purpose.

ALTER TABLE config_snapshots RENAME COLUMN aco_toml TO flowntier_toml;