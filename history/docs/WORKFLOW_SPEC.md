# Workflow Spec

> 8-phase state machine that drives every ACO workflow

**Version:** v0.1 RFC
**Status:** Draft
**Author:** Thatgfsj
**Supersedes:** PROJECT_SPEC.md §4 (formal state machine)
**Last updated:** 2026-06-18

---

## 1. Goals

1. Make every workflow **deterministic and replayable** from a single log.
2. Make every state transition **observable** and **interruptible**.
3. Bound every loop (planning, review, repair) with explicit **budgets**.
4. Separate **phases** (the user's mental model) from **states** (the
   machine's internal model).

---

## 2. Phases vs. States

* **Phase** = what the user sees on the timeline (PROJECT_SPEC §4)
* **State** = the machine's internal state (this document)

A phase typically contains 1–3 states. This separation lets us refactor
the machine without changing the timeline UI.

---

## 3. State Catalog

| State               | Phase             | Owner    | Description                              |
|---------------------|-------------------|----------|------------------------------------------|
| `REQ_RECEIVED`      | 1. Requirement    | Chief    | User input captured                      |
| `REQ_ANALYZING`     | 1. Requirement    | Chief    | Chief reading + clarifying              |
| `REQ_AWAIT_USER`    | 1. Requirement    | Chief    | Blocked on user (`USER_QUERY` sent)     |
| `REQ_CLARIFIED`     | 1. Requirement    | Chief    | User has answered; ready to plan         |
| `PLAN_DRAFTING`     | 2. Planning       | Chief    | Producing the planning document          |
| `PLAN_DRAFTED`      | 2. Planning       | Chief    | Plan doc exists; ready for review        |
| `PLAN_UNDER_REVIEW` | 3. Plan Review    | Critics  | Both critics reviewing                   |
| `PLAN_REVISING`     | 3. Plan Review    | Chief    | Chief addressing review feedback         |
| `PLAN_APPROVED`     | 3. Plan Review    | Chief    | Final plan locked; workers can spawn     |
| `DISPATCHING`       | 4. Dispatch       | Chief    | Issuing `TASK_ASSIGN` to each worker     |
| `DEVELOPING`        | 5. Development    | Workers  | Workers running in parallel              |
| `AWAITING_WORKERS`  | 5. Development    | Chief    | All `TASK_ASSIGN` sent; waiting for results |
| `REVIEWING`         | 6. Review         | Critics  | Reviewing aggregated deliverables        |
| `REPAIRING`         | 7. Repair         | Workers  | Sending `REPAIR_REQUEST` to relevant workers |
| `REWRITING`         | 7. Repair         | Chief    | Verdict was `REWRITE`; replan sub-tree   |
| `DELIVERING`        | 8. Delivery       | Chief    | Composing final summary for user         |
| `DONE`              | 8. Delivery       | —        | Terminal: success                        |
| `FAILED`            | —                 | Chief    | Terminal: unrecoverable error            |
| `ABORTED`           | —                 | —        | Terminal: user/system abort              |

**Terminal states:** `DONE`, `FAILED`, `ABORTED`. No transitions out.

---

## 4. Transition Table

| From                  | Event                         | To                       | Guard                                  |
|-----------------------|-------------------------------|--------------------------|----------------------------------------|
| `REQ_RECEIVED`        | start_analysis                | `REQ_ANALYZING`          | always                                 |
| `REQ_ANALYZING`       | need_clarification            | `REQ_AWAIT_USER`         | at least 1 open question               |
| `REQ_ANALYZING`       | analysis_done                 | `REQ_CLARIFIED`          | no open questions                      |
| `REQ_AWAIT_USER`      | user_responded                | `REQ_ANALYZING`          | always                                 |
| `REQ_AWAIT_USER`      | user_timeout                  | `FAILED`                 | 60 min idle                            |
| `REQ_CLARIFIED`       | start_planning                | `PLAN_DRAFTING`          | always                                 |
| `PLAN_DRAFTING`       | plan_emitted                  | `PLAN_DRAFTED`           | plan doc validated                     |
| `PLAN_DRAFTED`        | dispatch_review               | `PLAN_UNDER_REVIEW`      | always                                 |
| `PLAN_UNDER_REVIEW`   | both_critics_done             | `PLAN_REVISING` or `PLAN_APPROVED` | at least 1 issue → REVISING; else APPROVED |
| `PLAN_REVISING`       | plan_revised                  | `PLAN_DRAFTED`           | max 3 revisions                        |
| `PLAN_REVISING`       | max_revisions                 | `FAILED`                 | budget exhausted                       |
| `PLAN_APPROVED`       | start_dispatch                | `DISPATCHING`            | always                                 |
| `DISPATCHING`         | all_assigned                  | `AWAITING_WORKERS`       | all tasks in `DISPATCHED`              |
| `AWAITING_WORKERS`    | first_result_arrived          | `DEVELOPING`             | always                                 |
| `DEVELOPING`          | all_results_in                | `REVIEWING`              | all tasks terminal except approved     |
| `DEVELOPING`          | task_ask                      | `AWAITING_WORKERS`       | chief answering a `TASK_QUESTION`      |
| `DEVELOPING`          | repair_request                | `REPAIRING`              | any task in repair                     |
| `REVIEWING`           | verdict_pass                  | `DELIVERING`             | all tasks approved                     |
| `REVIEWING`           | verdict_repair                | `REPAIRING`              | at least 1 task needs repair           |
| `REVIEWING`           | verdict_rewrite               | `REWRITING`              | critic asked for rewrite               |
| `REPAIRING`           | all_repaired                  | `REVIEWING`              | max 3 repair loops                     |
| `REPAIRING`           | budget_exceeded               | `FAILED`                 | 3 repair loops done                    |
| `REWRITING`           | replan_done                   | `PLAN_REVISING`          | always                                 |
| `DELIVERING`          | report_emitted                | `DONE`                   | always                                 |
| any                   | user_abort                    | `ABORTED`                | always                                 |
| any                   | fatal_error                   | `FAILED`                 | uncaught exception                     |

---

## 5. State Diagram

```
                          user_abort / fatal_error
                                   │
                                   ▼
   ┌────────┐  start   ┌────────┐  need_clar  ┌────────┐
   │  REQ_  │────────▶│  REQ_  │────────────▶│  REQ_  │
   │RECEIVED│         │ANALYZ. │             │AWAIT_  │
   └────────┘         └───┬────┘             │ USER   │
       ▲                  │ analysis_done     └───┬────┘
       │ user_abort       ▼                       │ user_responded
       │              ┌────────┐                  │
       │              │  REQ_  │◀─────────────────┘
       │              │CLARIF. │
       │              └───┬────┘
       │                  │ start_planning
       │                  ▼
       │              ┌────────┐  plan_emitted  ┌────────┐
       │              │ PLAN_  │───────────────▶│ PLAN_  │
       │              │DRAFTING│                │DRAFTED │
       │              └────────┘                └───┬────┘
       │                                            │ dispatch_review
       │                                            ▼
       │                          ┌──────────────────────┐
       │                          │   PLAN_UNDER_REVIEW  │
       │                          └────────┬─────────────┘
       │                                   │ both_critics_done
       │              issues?              │
       │              ┌────┴────┐           │
       │              ▼         ▼           │
       │         ┌────────┐  ┌────────┐    │
       │         │ PLAN_  │  │ PLAN_  │    │
       │         │REVISING│  │APPROVED│    │
       │         └───┬────┘  └───┬────┘    │
       │             │plan_revised  │start_dispatch
       │             └──────┐       ▼
       │                    │   ┌─────────┐  all_assigned  ┌────────────┐
       │                    │   │DISPATCH.│───────────────▶│ AWAITING_  │
       │                    │   └─────────┘                │  WORKERS   │
       │                    │                             └─────┬──────┘
       │                    │                                   │ first_result
       │                    │                                   ▼
       │                    │                             ┌──────────┐
       │                    │                             │DEVELOPING│
       │                    │                             └────┬─────┘
       │                    │                                  │
       │                    │            all_results_in        │
       │                    │                                  ▼
       │                    │                             ┌──────────┐
       │                    │                             │ REVIEWING│
       │                    │                             └──┬───┬───┘
       │                    │            verdict_repair  │   │   │  verdict_pass
       │                    │                ┌───────────┘   │   └────────────┐
       │                    │                ▼               │                ▼
       │                    │         ┌──────────┐           │         ┌────────────┐
       │                    │         │REPAIRING │           │         │ DELIVERING │
       │                    │         └────┬─────┘           │         └──────┬─────┘
       │                    │              │ all_repaired   │                │ report
       │                    │              └──────┐         │                ▼
       │                    │                     ▼         │             ┌──────┐
       │                    │              (back to REVIEWING)            │ DONE │
       │                    │                                              └──────┘
       │                    │ verdict_rewrite
       │                    │              │
       │                    │              ▼
       │                    │         ┌──────────┐
       │                    │         │REWRITING │── replan_done ──▶ PLAN_REVISING
       │                    │         └──────────┘
       │                    │
       └────────────────────┴─── user_abort / fatal_error ────▶ ABORTED / FAILED
```

(ASCII simplified; see the transition table §4 for the source of truth.)

---

## 6. Budgets

Every workflow has these hard limits:

| Budget              | Default | Configurable | On exceed       |
|---------------------|---------|--------------|------------------|
| `max_plan_revisions`| 3       | yes          | → `FAILED`       |
| `max_repair_loops`  | 3       | yes          | → `FAILED`       |
| `max_total_tokens`  | 5M      | yes          | → `FAILED`       |
| `max_wallclock`     | 4 h     | yes          | → `FAILED`       |
| `max_parallel_workers` | 8    | yes          | queue, don't dispatch |
| `max_user_query_wait` | 60 min| yes          | → `FAILED`       |
| `max_workers`       | 32      | yes          | refuse new       |

Budgets are **per workflow run**, not global.

---

## 7. Persistence

Every state transition is appended to a single append-only log:

```json
{
  "ts":      "2026-06-18T12:34:56.789Z",
  "wf_id":   "wf_01HZX...",
  "from":    "PLAN_UNDER_REVIEW",
  "to":      "PLAN_REVISING",
  "event":   "both_critics_done",
  "actor":   "agent:chief",
  "context": { "issues_count": 3 }
}
```

File: `storage/workflows/<wf_id>.jsonl`

This log is the **replay format**. Given the log + the same models +
the same code, the workflow reproduces bit-for-bit (modulo non-determinism
in workers, which we accept).

---

## 8. Observability Hooks

The runtime emits an event on every transition. Subscribers (UI, metrics,
debugger) can listen:

```rust
pub enum WfEvent {
    Transition { from: State, to: State, event: Event },
    TokenUsage { agent: AgentId, usage: Usage },
    Console    { agent: AgentId, line: String },
    UserQuery  { query_id: QueryId, question: String },
    Milestone  { phase: Phase, label: String },  // for UI
}
```

The UI subscribes to `Milestone` events to update the timeline (see
[UI_GUIDELINES.md §3 T0](./UI_GUIDELINES.md)). The other events feed
the bottom console and the debug log.

---

## 9. Cancellation & Resume

### 9.1 User cancel

* User clicks "Stop" → runtime sets a `cancel_requested` flag.
* Current state finishes its current unit of work (one message round-trip
  for Chief, one `TASK_RESULT` for Worker).
* Workflow transitions to `ABORTED`.
* All spawned Workers receive `ABORT`.
* Disk log is sealed; user can inspect.

### 9.2 Crash recovery

* Every transition is fsync'd before the next one is accepted.
* On startup, the runtime scans `storage/workflows/*.jsonl` and detects
  runs whose last entry is **not** a terminal state.
* For each such run, it offers the user: **Resume** / **Discard** / **Inspect**.
* Resume replays from the last stable state using the persisted context.

---

## 10. Concurrency Model

* One workflow = one Chief process/thread.
* Workers are **not threads inside Chief**. They are **async tasks** in
  the same runtime, communicating via the in-process queue
  (see [AGENT_PROTOCOL §9](./AGENT_PROTOCOL.md)).
* Critics run as async tasks spawned on demand.
* The runtime is single-process per workflow in v0.1. Multi-workflow
  parallelism is a v0.3 feature.

---

## 11. Determinism Guarantees

* Replaying the same log with the same models and same code produces
  the **same final state**, with these caveats:
  * Worker outputs may differ slightly if temperature > 0.
  * External state (filesystem, network) is captured as `artifacts` in
    `TASK_RESULT` and re-applied during replay.
* For replay to be **bit-exact**, the user must set `temperature: 0` in
  the model spec.

---

## 12. Open Questions

1. Should `REWRITING` always go back through `PLAN_REVISING`, or directly
   to a partial re-plan? (proposed: through `PLAN_REVISING` for v0.1
   simplicity, optimize in v0.2)
2. Should we let users **inject a partial plan** mid-workflow? (proposed:
   no in v0.1, yes in v0.3 — "guided mode")
3. Should `REVIEWING` use both critics in parallel, or sequentially?
   (proposed: parallel, with a configurable `critic_b_optional: true`)

---

**RFC ends.**
