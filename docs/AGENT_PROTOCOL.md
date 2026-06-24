# Agent Protocol

> Inter-agent communication contract for Flowntier

**Version:** v0.3 RFC
**Status:** Active
**Author:** Thatgfsj
**Last updated:** 2026-06-22

---

## 1. Goals

Define a **single, versioned, machine-readable contract** that all
agents in Flowntier use to talk to each other. The contract must:

1. Be transport-agnostic (in-process queue, Redis, HTTP, Unix socket — all OK)
2. Be strictly **envelope-shaped** so unknown fields don't break parsers
3. Guarantee **isolation**: workers have no peer channel
4. Be **forward-compatible**: a v0.1 client can talk to a v0.2 server (additive only)
5. Be **human-debuggable**: every message can be pretty-printed

---

## 2. Actors

| 中文名 | 协议名 | Talks to | Talks with |
|--------|--------|----------|------------|
| **主理** (Chief) | `agent:chief` | 找茬 / 审查 / 实施 / 计划 / 汇报 / User | All |
| **找茬** (Critic A) | `agent:critic:a` | 主理 | 主理 only |
| **审查** (Critic B) | `agent:critic:b` | 主理 | 主理 only |
| **计划** (Planner) | `agent:planner` | 主理 | 主理 only |
| **实施** (Worker) | `agent:worker:<task-slug>` | 主理 | 主理 only |
| **汇报** (Reporter) | `agent:reporter` | 主理 | 主理 only |
| **User** | `agent:user` | 主理 | 主理 only |

**Rule:** Only the Chief is a hub. All other agents are **leaves**.

There is **no** Critic↔Critic channel, **no** Worker↔Worker channel, and
**no** Worker↔Critic direct channel. All such interactions go through
the Chief.

---

## 3. Message Envelope

Every message — regardless of type — has the same outer shape:

```json
{
  "id":      "msg_01HZX3R8K7Q9F2N5M4B6V8C0XA",  // ULID, unique
  "schema":  "agent-protocol/v0.1",            // protocol version
  "from":    "agent:chief",                    // sender id
  "to":      "agent:worker:backend-login",     // recipient id
  "type":    "TASK_ASSIGN",                    // see §4
  "ts":      "2026-06-18T12:34:56.789Z",       // ISO 8601 UTC
  "trace":   "wf_01HZX3...:phase:developing",  // workflow + phase context
  "payload": { /* type-specific, see §4 */ }
}
```

### 3.1 Field rules

| Field    | Type        | Required | Notes                                         |
|----------|-------------|----------|-----------------------------------------------|
| `id`     | string      | yes      | ULID. Used for idempotency / dedup.           |
| `schema` | string      | yes      | `agent-protocol/<major>.<minor>`. Major bump = breaking. |
| `from`   | agent-id    | yes      | See §3.2                                      |
| `to`     | agent-id    | yes      | See §3.2. `agent:chief` is the only valid peer for workers/critics. |
| `type`   | enum        | yes      | One of §4.                                    |
| `ts`     | ISO 8601    | yes      | UTC, ms precision                             |
| `trace`  | string      | yes      | `wf_<ulid>:phase:<name>` — for correlation     |
| `payload`| object      | yes      | Type-specific. Must be JSON-serializable.     |

Unknown top-level fields **must** be ignored by receivers (forward compat).

### 3.2 Agent ID grammar

```
agent:<role>[:<instance-id>]
```

| Pattern                              | Meaning                       |
|--------------------------------------|-------------------------------|
| `agent:chief`                        | The Chief (singleton)         |
| `agent:critic:a` / `agent:critic:b`  | A specific Critic (singleton) |
| `agent:worker:<task-slug>`           | A spawned Worker              |
| `agent:user`                         | The human                     |
| `agent:reporter`                     | The summary agent (v0.2)      |

`<task-slug>` is a kebab-case identifier, e.g. `backend-login`.

### 3.3 Reserved agent IDs

* `agent:system` — internal control messages (e.g., shutdown)
* `agent:*:broadcast` — not allowed in v0.1 (no fan-out)

---

## 4. Message Types

| Type                 | From → To           | Purpose                              |
|----------------------|---------------------|--------------------------------------|
| `TASK_ASSIGN`        | Chief → Worker      | Hand a worker a task                  |
| `TASK_PROGRESS`      | Worker → Chief      | Heartbeat / partial update            |
| `TASK_RESULT`        | Worker → Chief      | Completed (or failed) deliverable     |
| `TASK_QUESTION`      | Worker → Chief      | Worker is blocked, needs clarification |
| `REVIEW_REQUEST`     | Chief → Critic      | Ask for review of a deliverable       |
| `REVIEW_RESPONSE`    | Critic → Chief      | Verdict + issues                      |
| `REPAIR_REQUEST`     | Chief → Worker      | Send a worker back to fix issues      |
| `DISPATCH_PLAN`      | Chief → Workers (sequential, one msg each) | Plan + interface contracts |
| `ESCALATE`           | Worker/Critic → Chief | Cannot proceed, surface to user     |
| `USER_QUERY`         | Chief → User        | Ask the user something                |
| `USER_RESPONSE`      | User → Chief        | User's answer                         |
| `USER_FEEDBACK`      | User → Chief        | Free-form feedback ("more detail")    |
| `ABORT`              | Chief → any         | Cancel a task mid-flight              |
| `SHUTDOWN`           | system → any        | Graceful shutdown                     |

---

## 5. Payloads (canonical schemas)

### 5.1 `TASK_ASSIGN`

```json
{
  "task_id":  "task_01HZX...",
  "title":    "Implement /login endpoint",
  "objective": "Accept JSON {email, password}, return JWT or 401.",
  "interfaces": {
    "consumes": ["POST /auth/login", "users table"],
    "produces": ["JWT (HS256, 24h)", "audit log entry"]
  },
  "dependencies": [
    "task_01HZY...:database-users"
  ],
  "constraints": [
    "Use bcrypt cost 12",
    "Rate-limit to 5 req/min/IP",
    "No third-party auth libraries"
  ],
  "deliverables": [
    "src/auth/login.py",
    "src/auth/login.test.py"
  ],
  "context_budget_tokens": 16000,
  "model_hint": "minimax-m3"
}
```

A Worker **must not** receive anything outside this envelope. In particular:
* no project-wide context
* no other workers' outputs
* no critic opinions

### 5.2 `TASK_PROGRESS`

```json
{
  "task_id":  "task_01HZX...",
  "pct":      42,
  "note":     "Wrote login route, running tests"
}
```

Sent at most every 5 s. Receiver should not act on it (informational only).

### 5.3 `TASK_RESULT`

```json
{
  "task_id":  "task_01HZX...",
  "status":   "DONE" | "FAILED" | "PARTIAL",
  "summary":  "Implemented /login; 12 tests pass.",
  "files_modified": [
    {"path": "src/auth/login.py",   "lines_added": 87, "lines_removed": 3},
    {"path": "src/auth/login.test.py", "lines_added": 64, "lines_removed": 0}
  ],
  "tests_run":  {"passed": 12, "failed": 0, "skipped": 0},
  "artifacts":  ["dist/build/login.wasm"],
  "logs_ref":   "console:task_01HZX:0..142"
}
```

### 5.4 `TASK_QUESTION`

```json
{
  "task_id":  "task_01HZX...",
  "question": "Spec says 'JWT or 401' — what status on rate limit exceeded?",
  "options":  ["429 Too Many Requests", "401 Unauthorized", "Either is fine — your call"]
}
```

Chief must either answer (re-emit as `TASK_ASSIGN` with a refined `objective`)
or escalate to the user as `USER_QUERY`.

### 5.5 `REVIEW_REQUEST`

```json
{
  "review_id":  "rev_01HZX...",
  "subject":    "Phase 4 deliverable: /login implementation",
  "files":      ["src/auth/login.py", "src/auth/login.test.py"],
  "diff_ref":   "git:abc1234..def5678",
  "ask":        "Check for runtime bugs, edge cases, security.",
  "criteria":   ["no unhandled exceptions", "no SQL injection", "rate-limit enforced"]
}
```

### 5.6 `REVIEW_RESPONSE`

```json
{
  "review_id":  "rev_01HZX...",
  "verdict":    "PASS" | "REPAIR" | "REWRITE",
  "confidence": 0.87,
  "issues": [
    {
      "severity": "MAJOR" | "MINOR" | "NIT",
      "file":     "src/auth/login.py",
      "line":     42,
      "message":  "bcrypt cost is 10, spec requires 12",
      "suggested_fix": "bcrypt.hashpw(pw, bcrypt.gensalt(rounds=12))"
    }
  ],
  "summary": "One MAJOR issue. Otherwise solid."
}
```

### 5.7 `REPAIR_REQUEST`

```json
{
  "task_id":  "task_01HZX...",
  "issues":   [ /* same shape as REVIEW_RESPONSE.issues */ ],
  "retain":   ["src/auth/login.test.py"],   // files not to touch
  "deadline": "2026-06-18T13:00:00Z"
}
```

### 5.8 `USER_QUERY`

```json
{
  "question": "Do you want OAuth support in v0.1, or email/password only?",
  "options":  ["Email/password only", "Add Google OAuth", "Add Google + GitHub"]
}
```

### 5.9 `USER_RESPONSE`

```json
{
  "answer":   "Email/password only",
  "freeform": "We can add OAuth in v0.2"
}
```

### 5.10 `ABORT`

```json
{
  "task_id": "task_01HZX...",
  "reason":  "user_canceled" | "timeout" | "dependency_failed" | "internal_error"
}
```

---

## 6. Task Lifecycle

```
                  ┌──────────┐
                  │ PENDING  │  (in Chief's queue)
                  └────┬─────┘
                       │ TASK_ASSIGN
                       ▼
                  ┌──────────┐
                  │DISPATCHED│  (sent, not yet started)
                  └────┬─────┘
                       │ worker starts
                       ▼
                  ┌──────────────┐
       ┌──────────│IN_PROGRESS   │
       │          └──────┬───────┘
       │                 │ TASK_RESULT
       │                 ▼
       │          ┌──────────────┐
       │          │  SUBMITTED   │
       │          └──────┬───────┘
       │                 │ Chief dispatches to Critic
       │                 ▼
       │          ┌──────────────┐
       │          │UNDER_REVIEW  │
       │          └──────┬───────┘
       │                 │ REVIEW_RESPONSE
       │     ┌───────────┼─────────────┐
       │     ▼           ▼             ▼
       │  ┌──────┐  ┌──────────┐  ┌─────────┐
       │  │APPR. │  │ REPAIR_  │  │REJECTED │  (REWRITE verdict)
       │  │      │  │REQUESTED │  └────┬────┘
       │  └──┬───┘  └────┬─────┘       │
       │     │           │ REPAIR_REQUEST
       │     │           ▼
       │     │      ┌──────────┐
       │     │      │ REPAIRING│
       │     │      └────┬─────┘
       │     │           │ TASK_RESULT (re-submit)
       │     │           └─→ SUBMITTED
       │     ▼
       │  ┌──────┐
       │  │ DONE │  ◀── final
       │  └──────┘
       │
       └── ABORT → ┌────────┐
                   │ ABORTED│  ◀── final
                   └────────┘
```

* **Repair budget:** max 3 repair loops per task. Exceeding → escalate to user.
* **Timeout:** each state has a configurable max dwell time (see §8).
* **Idempotency:** re-delivering a `TASK_ASSIGN` with the same `task_id` is
  a no-op for the worker (it must dedup on `id`).

---

## 7. Isolation Guarantees

1. A Worker **never** receives a `from` other than `agent:chief`.
2. A Worker **cannot** address a `to` other than `agent:chief`. The runtime
   must reject any such message at the transport layer.
3. A Worker has **no** reference to other Worker IDs. The Chief must never
   leak one into a `TASK_ASSIGN.payload`.
4. Critics receive only the deliverable under review, never the raw
   multi-worker context.

Violations are **bugs**, not features. The runtime must enforce (1)–(4).

---

## 8. Timeouts

| State           | Default timeout | On expiry                          |
|-----------------|----------------|------------------------------------|
| `DISPATCHED`    | 60 s           | ABORT(reason=`timeout`)            |
| `IN_PROGRESS`   | 30 min         | send reminder, then ABORT at +10m  |
| `UNDER_REVIEW`  | 10 min         | send reminder, then ABORT at +10m  |
| `REPAIRING`     | 20 min         | escalate to user                   |
| `PENDING`       | 60 min         | escalate to user                   |

All timeouts are **per-task overridable** via `TASK_ASSIGN.context_budget_tokens`
indirectly (token-budget exhaustion ⇒ timeout).

---

## 9. Transport

v0.1 supports exactly **one** transport:

* **In-process queue** (Rust `tokio::mpsc` / Python `asyncio.Queue`)

v0.2 will add:

* Unix domain socket (Linux/macOS) / named pipe (Windows)
* HTTP+JSON-RPC for cross-host workers (rare; for very large workflows)

**Recommendation:** keep message format transport-agnostic, even in v0.1.

---

## 10. Versioning

* Protocol version is in the `schema` field: `agent-protocol/v0.1`.
* **Minor** bump (`v0.1` → `v0.2`): additive only. Receivers must ignore unknown fields/types.
* **Major** bump (`v0.x` → `v1.0`): may remove fields. Receivers must reject
  on unknown `schema` major.
* Deprecated message types must be accepted (no-op) for **at least 2 minor
  versions** before removal.

---

## 11. Error Handling

Every message is one-shot and unidirectional. There is **no** automatic
retry by default.

| Failure                       | Behavior                                  |
|-------------------------------|-------------------------------------------|
| Worker crashes mid-task       | Chief sees missing heartbeat, ABORT       |
| Chief crashes mid-workflow    | Workflow is persisted; resume on restart  |
| Critic gives non-conformant reply | Chief logs + retries once, then ESCLATE |
| Malformed message             | Receiver logs + drops; sender is not retried (caller's problem) |

---

## 12. v0.3 Implementation Map

This protocol is **implementation-agnostic**. v0.3 implements it as
follows:

| Protocol concept | v0.3 implementation |
|------------------|---------------------|
| Envelope (the JSON above) | `crates/agent-core/src/protocol.rs::Envelope` (Rust serde) |
| Transport | In-process `tokio::mpsc` between role handlers in `crates/agent-core/src/loop.rs` |
| Agent IDs | `主理::id() = "agent:chief"`, etc., derived from role |
| Persistence | Each envelope appended to `storage/workflows/<wf_id>.jsonl` |
| UI surface | Streamed to webview via `crates/event-bus` → Tauri `wf:event` |

**Removed as of v0.3:**

* The Python `runtime/aco_runtime_lib/agents/` package. The Chief /
  Critic / Worker classes there are now Rust structs in
  `crates/agent-core/src/roles/`.
* The Claude Code CLI adapter (`crates/claude-adapter/`) — replaced
  by `crates/agent-core/src/providers/` (HTTPS SSE) and
  `crates/agent-core/src/tools/` (in-process execution).
* The `portable-pty` dependency. Subprocess execution goes through
  `tokio::process` inside the `bash` tool only.

The **protocol** itself is unchanged: same envelope, same types,
same isolation rules. Only the implementation language and runtime
location changed.

---

## 13. Open Questions

1. Should `TASK_QUESTION` block the worker, or be best-effort? (proposed: blocking)
2. Should `USER_QUERY` have a default option (timeout → pick first option)? (proposed: yes, with a "no default — abort" override)
3. Should we sign messages (HMAC) for tamper detection? (proposed: not in v0.3)
4. v0.3 deprecates the English role names (`Chief`, `Critic`, …) in
   favor of 中文 (`主理`, `找茬`, …). Should we keep the protocol
   field `from: "agent:chief"` for backward compat, or change to
   `from: "agent:shouxi"`? (proposed: keep `agent:chief` for wire
   compat; map to 中文 only in UI)

---

**RFC ends.**
