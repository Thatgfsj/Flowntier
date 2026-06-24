# Storage Spec

> SQLite schema, FTS5 indices, JSONL workflow log, backup/recovery

**Version:** v0.1 RFC
**Status:** Draft
**Author:** Thatgfsj
**Related:** [ARCHITECTURE.md](./ARCHITECTURE.md) · [WORKFLOW_SPEC.md](./WORKFLOW_SPEC.md)
**Last updated:** 2026-06-18

---

## 1. Goals

1. **One SQLite file per user.** No server, no daemon.
2. **Append-only for the hot path** (workflow logs); mutable for
   cold data (project memory, plugin registry).
3. **FTS5** for fast full-text search across workflows, console
   lines, and prompt history.
4. **Recoverable**: corruption, partial writes, and crashes must
   not lose user data.
5. **Migratable**: schema versions are tracked; migrations are
   forward-only and idempotent.

---

## 2. Location

| OS       | Path                                                   |
|----------|--------------------------------------------------------|
| Windows  | `%APPDATA%\aco\storage.sqlite`                          |
| macOS    | `~/Library/Application Support/aco/storage.sqlite`     |
| Linux    | `~/.config/aco/storage.sqlite`                         |

WAL files (`storage.sqlite-wal`, `storage.sqlite-shm`) live alongside.
Workflow JSONL logs live in `$ACO_DATA/workflows/<wf_id>.jsonl`.

---

## 3. SQLite Configuration

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous  = NORMAL;     -- safety vs. speed: NORMAL is fine with WAL
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;        -- ms
PRAGMA temp_store   = MEMORY;
PRAGMA mmap_size    = 268435456;   -- 256 MB
```

Connection pool: max 1 writer, 8 readers (SQLx default).

---

## 4. Schema (v0.1)

### 4.1 `workflows` — top-level workflow metadata

```sql
CREATE TABLE workflows (
  id              TEXT PRIMARY KEY,           -- ULID
  created_at      INTEGER NOT NULL,           -- unix seconds
  updated_at      INTEGER NOT NULL,
  state           TEXT NOT NULL,              -- see WORKFLOW_SPEC §3
  phase           TEXT NOT NULL,              -- see WORKFLOW_SPEC §3
  user_request    TEXT NOT NULL,              -- original input
  plan_doc        TEXT,                       -- markdown
  summary         TEXT,                       -- final delivery summary
  final_status    TEXT,                       -- DONE | FAILED | ABORTED
  total_input_tokens  INTEGER NOT NULL DEFAULT 0,
  total_output_tokens INTEGER NOT NULL DEFAULT 0,
  total_cost_usd REAL
);

CREATE INDEX idx_workflows_created ON workflows(created_at DESC);
CREATE INDEX idx_workflows_state   ON workflows(state);
```

### 4.2 `workflow_log` — append-only event log (mirror of JSONL)

The JSONL file is the **source of truth**; this table is a derived
index for fast queries.

```sql
CREATE TABLE workflow_log (
  wf_id       TEXT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
  seq         INTEGER NOT NULL,                -- monotonic per wf_id
  ts          INTEGER NOT NULL,
  from_state  TEXT,
  to_state    TEXT,
  event       TEXT,
  actor       TEXT,
  context     TEXT,                            -- JSON blob
  PRIMARY KEY (wf_id, seq)
);

CREATE INDEX idx_log_ts ON workflow_log(wf_id, ts);
```

### 4.3 `tasks` — per-workflow task records

```sql
CREATE TABLE tasks (
  id              TEXT PRIMARY KEY,           -- ULID
  wf_id           TEXT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
  parent_id       TEXT REFERENCES tasks(id),  -- for sub-tasks
  title           TEXT NOT NULL,
  status          TEXT NOT NULL,              -- PENDING | DISPATCHED | IN_PROGRESS
                                                -- | SUBMITTED | UNDER_REVIEW
                                                -- | APPROVED | REPAIR_REQUESTED
                                                -- | REPAIRING | REJECTED
                                                -- | DONE | FAILED | ABORTED
  assigned_to     TEXT,                       -- agent:<...>
  model           TEXT,                       -- provider:model
  repair_count    INTEGER NOT NULL DEFAULT 0,
  input_tokens    INTEGER NOT NULL DEFAULT 0,
  output_tokens   INTEGER NOT NULL DEFAULT 0,
  cost_usd        REAL,
  files_modified  TEXT,                       -- JSON array
  started_at      INTEGER,
  finished_at     INTEGER,
  result          TEXT                        -- JSON blob
);

CREATE INDEX idx_tasks_wf ON tasks(wf_id);
CREATE INDEX idx_tasks_status ON tasks(status);
```

### 4.4 `usage` — token/cost ledger

```sql
CREATE TABLE usage (
  id              TEXT PRIMARY KEY,
  ts              INTEGER NOT NULL,
  task_id         TEXT REFERENCES tasks(id),
  wf_id           TEXT REFERENCES workflows(id) ON DELETE CASCADE,
  agent_id        TEXT NOT NULL,
  provider        TEXT NOT NULL,
  model           TEXT NOT NULL,
  input_tokens    INTEGER NOT NULL,
  output_tokens   INTEGER NOT NULL,
  cached_tokens   INTEGER NOT NULL DEFAULT 0,
  cost_usd        REAL,
  finish_reason   TEXT
);

CREATE INDEX idx_usage_ts   ON usage(ts);
CREATE INDEX idx_usage_task ON usage(task_id);
CREATE INDEX idx_usage_wf   ON usage(wf_id);
```

### 4.5 `prompts` — prompt history (for replay/A-B)

```sql
CREATE TABLE prompts (
  id              TEXT PRIMARY KEY,
  ts              INTEGER NOT NULL,
  wf_id           TEXT REFERENCES workflows(id) ON DELETE CASCADE,
  task_id         TEXT REFERENCES tasks(id),
  agent_id        TEXT NOT NULL,
  role            TEXT NOT NULL,              -- system | user | assistant | tool
  content         TEXT NOT NULL,
  model           TEXT,
  input_tokens    INTEGER,
  output_tokens   INTEGER
);
```

### 4.6 `config_snapshots` — per-workflow config used

```sql
CREATE TABLE config_snapshots (
  wf_id       TEXT PRIMARY KEY REFERENCES workflows(id) ON DELETE CASCADE,
  aco_toml    TEXT NOT NULL,
  providers   TEXT NOT NULL,                  -- JSON
  router      TEXT NOT NULL                   -- JSON
);
```

### 4.7 `project_memory` — project-scoped facts

```sql
CREATE TABLE project_memory (
  id          TEXT PRIMARY KEY,
  project_id  TEXT NOT NULL,                  -- per .aco/config.yaml
  key         TEXT NOT NULL,
  value       TEXT NOT NULL,                  -- JSON or text
  source      TEXT,                           -- which agent wrote it
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL,
  UNIQUE (project_id, key)
);

CREATE INDEX idx_memory_project ON project_memory(project_id);
```

### 4.8 `plugins` — installed plugin registry

```sql
CREATE TABLE plugins (
  id          TEXT PRIMARY KEY,               -- matches plugin.toml [plugin].id
  version     TEXT NOT NULL,
  path        TEXT NOT NULL,                  -- absolute
  manifest    TEXT NOT NULL,                  -- raw plugin.toml
  state       TEXT NOT NULL,                  -- DISCOVERED | VALIDATED
                                                -- | INITIALIZED | ENABLED
                                                -- | DISABLED | UNLOADED
                                                -- | BROKEN
  enabled_at  INTEGER,
  last_error  TEXT
);
```

### 4.9 FTS5 — full-text search

```sql
CREATE VIRTUAL TABLE search_idx USING fts5(
  kind UNINDEXED,           -- 'workflow' | 'task' | 'log' | 'prompt' | 'memory'
  ref_id UNINDEXED,         -- primary id
  wf_id UNINDEXED,
  content,
  tokenize = 'porter unicode61 remove_diacritics 2'
);
```

Populated by triggers on `workflows.user_request`, `tasks.title`,
`workflow_log.context`, `prompts.content`, `project_memory.value`.

---

## 5. Schema Migrations

* `migrations/` directory; one file per version: `0001_init.sql`,
  `0002_add_memory.sql`, …
* Each migration is forward-only and idempotent.
* Applied by SQLx's `migrate!` macro at app startup.
* **Never** edit a migration after it's been released — write a new one.

```rust
sqlx::migrate!("./migrations").run(&pool).await?;
```

Migration table:

```sql
CREATE TABLE schema_migrations (
  version     INTEGER PRIMARY KEY,
  description TEXT NOT NULL,
  installed_at INTEGER NOT NULL
);
```

---

## 6. Workflow JSONL Log (Source of Truth)

Every state transition is **also** written to
`$ACO_DATA/workflows/<wf_id>.jsonl`, one JSON object per line.

```jsonl
{"ts":"2026-06-18T12:34:56.789Z","wf_id":"wf_01...","seq":1,"from":null,"to":"REQ_RECEIVED","event":"user_input","actor":"agent:user","context":{"text":"Build me a /login endpoint"}}
{"ts":"...","wf_id":"...","seq":2,"from":"REQ_RECEIVED","to":"REQ_ANALYZING","event":"start_analysis","actor":"agent:chief","context":{}}
```

* `fsync` after every line (durability).
* The SQLite `workflow_log` table is built by a **background tailer**
  on startup, then kept in sync via in-process events.
* Replay reads this file only — SQLite is the query cache.

---

## 7. Backup & Recovery

### 7.1 Automatic

* On every clean shutdown, the runtime runs:
  ```sql
  PRAGMA wal_checkpoint(TRUNCATE);
  ```
* Daily: copy `storage.sqlite` to `backups/storage-<yyyy-mm-dd>.sqlite`.
  Keep the last **7** daily + **4** weekly backups.

### 7.2 Corruption detection

* On startup, run `PRAGMA integrity_check`.
* On failure, attempt a `VACUUM INTO backups/recovered-<ts>.sqlite`,
  then surface a UI prompt: restore from backup or proceed with
  recovered copy.

### 7.3 Crash recovery (workflow)

* On startup, scan `workflows/*.jsonl` for files whose last entry
  is not a terminal state.
* For each: prompt the user to **Resume**, **Discard**, or **Inspect**.
* Resume replays from the last stable state (see
  [WORKFLOW_SPEC §9.2](./WORKFLOW_SPEC.md)).

### 7.4 User-initiated export

* Settings → Storage → "Export everything" produces a `.tar.gz`:
  * `storage.sqlite`
  * `workflows/*.jsonl`
  * `prompts/<role>/<version>/*.md` (the active set)
  * `aco.toml`, `providers.toml`, `router.toml`
* Import is the inverse; the runtime validates schema version first.

---

## 8. Encryption at Rest (v1.0, **not** v0.1)

* v0.1: **no** encryption at rest. API keys live in env vars; workflow
  logs are plaintext (they don't contain secrets).
* v1.0: optional AES-256-GCM passphrase, key derived via Argon2id.
  Encrypted blob written to `storage.sqlite.enc`; the unencrypted
  file is removed after a successful re-encrypt.

**Why not v0.1:** encryption is a footgun for a local-first product
that hasn't been audited; secrets live elsewhere (env vars); and
losing the passphrase means losing the workspace. Defer until v1.0.

---

## 9. Retention Policy

| Data           | Default retention | Configurable |
|----------------|-------------------|--------------|
| Workflow JSONL | Forever           | yes (per-wf delete) |
| `usage` rows   | 12 months         | yes          |
| `prompts` rows | 6 months          | yes          |
| Daily backups  | 7                 | yes          |
| Weekly backups | 4                 | yes          |
| Plugin broken-state | 30 days, then auto-uninstalled | yes |

User can pin a workflow to "never delete" via a star icon in the UI.

---

## 10. Concurrency Rules

* **One writer at a time.** `BEGIN IMMEDIATE` for any transaction
  that may write; the rest use `BEGIN DEFERRED`.
* **Long-running reads** (search, dashboard) use a separate read
  connection with `PRAGMA query_only = ON`.
* **No nested transactions.** Use savepoints when needed.

---

## 11. Open Questions

1. Should we **shard** `usage` by month (separate tables) once it
   exceeds ~10M rows? (proposed: yes, automatic at v0.3)
2. Should FTS5 use a **porter stemmer** or stay unstemmed for code
   search? (proposed: separate `search_idx_code` without stemming)
3. Should we **journal** the entire workflow to disk on every
   transition (durability) or batch every 1s (perf)? (proposed:
   journal on every transition — safety wins)

---

**RFC ends.**
