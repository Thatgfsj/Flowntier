-- 0005_quota_failures.sql
-- v0.4.20 (event 000056): per-(role, model) quota-failure tracker.
-- Each run_task failure is recorded here; a background scheduler
-- in pipe-server ticks every minute and, on 5-hour boundaries
-- (00:01, 05:01, 10:01, 15:01, 20:01), retries any row that is
-- status='pending_5h_wait'. If the retry fails the row flips to
-- status='rate_limited' and a structured system log fires; if it
-- succeeds the row is deleted.
--
-- One row per (role_id, model_id). Auto-cleaned on success.

CREATE TABLE IF NOT EXISTS quota_failures (
    role_id            TEXT    NOT NULL,
    model_id           TEXT    NOT NULL,
    last_error_at      INTEGER NOT NULL,
    last_error_message TEXT    NOT NULL DEFAULT '',
    status             TEXT    NOT NULL DEFAULT 'failed',
        -- 'failed' | 'pending_5h_wait' | 'rate_limited'
    attempt_count      INTEGER NOT NULL DEFAULT 0,
    next_attempt_at    INTEGER,
    PRIMARY KEY (role_id, model_id)
);

-- Partial index keeps the scheduler SELECT cheap: the only rows the
-- 5h-tick scans are pending ones.
CREATE INDEX IF NOT EXISTS idx_quota_pending
    ON quota_failures (next_attempt_at)
    WHERE status = 'pending_5h_wait';
