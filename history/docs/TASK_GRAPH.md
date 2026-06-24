# Task Graph

> Task graph data structure, scheduling, dependencies, parallelism

**Version:** v0.1 RFC
**Status:** Draft
**Author:** Thatgfsj
**Related:** [WORKFLOW_SPEC.md](./WORKFLOW_SPEC.md) · [AGENT_PROTOCOL.md](./AGENT_PROTOCOL.md)
**Last updated:** 2026-06-18

---

## 1. Goals

1. Make a plan **inspectable** — the user must see the whole graph
   before workers start.
2. Make scheduling **deterministic** — given the same graph and
   the same model outputs, two runs dispatch the same way.
3. Bound parallelism — never flood the LLM providers, never
   exceed `max_parallel_workers`.
4. Support **repair** — when a task fails, only re-dispatch the
   affected sub-graph, not the whole plan.

---

## 2. Data Model

A plan is a **DAG** of `TaskNode`s.

```rust
pub struct Plan {
    pub id: PlanId,
    pub workflow_id: WorkflowId,
    pub title: String,
    pub goal: String,
    pub nodes: Vec<TaskNode>,
    pub edges: Vec<Edge>,
    pub acceptance: Vec<AcceptanceCriterion>,
    pub risks: Vec<Risk>,
    pub out_of_scope: Vec<String>,
}

pub struct TaskNode {
    pub id: TaskId,                 // ULID
    pub slug: String,               // kebab-case, e.g. "backend-login"
    pub title: String,
    pub owner_role: Role,           // Backend | Frontend | Database | ...
    pub depends_on: Vec<TaskId>,    // empty for roots
    pub interfaces: Interfaces,     // consumes / produces
    pub constraints: Vec<String>,
    pub deliverables: Vec<PathBuf>,
    pub est_input_tokens: u32,
    pub est_output_tokens: u32,
    pub status: TaskStatus,         // see WORKFLOW_SPEC §6
}

pub struct Edge {
    pub from: TaskId,
    pub to: TaskId,
    pub kind: EdgeKind,             // Hard | Soft
}

pub enum EdgeKind {
    Hard,   // `to` cannot start until `from` is DONE
    Soft,   // `to` may start without `from`; will merge later
}
```

**Invariant:** the graph is acyclic. Plan validation rejects cycles.

---

## 3. Plan Lifecycle

```
Chief drafts plan (Markdown) ──▶ Parse ──▶ Validate (DAG, deps) ──▶ Lock
                                                                    │
Critic A review ──────────────────────────────────────────────────┐ │
Critic B review ──────────────────────────────────────────────────┘ │
                                                                    ▼
                                                          Dispatch loop
```

### 3.1 Parse

The plan doc (Markdown from the Chief) is parsed into the `Plan`
struct by `runtime/workflow/plan_parser.py`. The parser is **strict**:
unknown sections or malformed tables are a parse error, not a
warning.

### 3.2 Validate

* No cycles (topological sort must succeed).
* Every `depends_on` reference points to a node in the same plan.
* Every `deliverables` path is a forward-slash relative path.
* Every `owner_role` is one of the known roles.
* `est_input_tokens + est_output_tokens <= budget_per_task`.

Failures are returned as `PlanValidationError` to the Chief, who
must revise.

### 3.3 Lock

Once locked, the graph is **immutable**. Any change goes through a
new plan (or a `REWRITE` verdict that triggers `REWRITING`).

---

## 4. Scheduler

The scheduler picks the next batch of ready tasks.

### 4.1 Definitions

* **Ready** = `status == PENDING` AND every `depends_on` task is
  in a terminal-success state (`DONE` or `APPROVED`).
* **Runnable** = `Ready` AND the runtime has spare worker slots
  (`active_workers < max_parallel_workers`).

### 4.2 Algorithm (per tick, every 1s)

```python
def tick(graph, runtime):
    # 1. Update statuses from in-flight results
    apply_pending_results(graph, runtime)

    # 2. Find ready tasks, sorted by:
    #    (a) topological order
    #    (b) within a level, by est_total_tokens ascending
    ready = sorted(find_ready(graph), by=key)

    # 3. Dispatch up to (max_parallel_workers - active) tasks
    slots = runtime.max_parallel_workers - runtime.active_count()
    for task in ready[:slots]:
        runtime.dispatch(task)
        graph[task].status = DISPATCHED
```

### 4.3 Properties

* **Determinism**: same graph + same dispatch order = same outcomes.
* **Fairness**: shorter tasks run first within a topological level
  (lower wallclock; less idle workers at the end).
* **Bounded**: never dispatches more than `max_parallel_workers`
  at once.

### 4.4 Repair scheduling

When a `REPAIR_REQUEST` is sent:

1. The repaired task's status → `REPAIRING`.
2. Tasks that **depend on** the repaired task are reset to `PENDING`
   (if not already done) so they re-run after the repair.
3. Tasks that **depended transitively only on the repaired path**
   are also reset.

The scheduler then re-evaluates the ready set on the next tick.

---

## 5. Parallelism Limits

| Limit                | Default | Configurable |
|----------------------|---------|--------------|
| `max_parallel_workers` | 8      | yes (1–32)   |
| `max_tasks_per_minute` | 30    | yes (1–120)  |
| `max_tokens_per_minute`| 2M    | yes (per-provider) |
| `max_concurrent_per_model` | 4 | yes          |

The scheduler **always** checks these before dispatch. A task that
would violate a limit stays in `PENDING` until the next tick.

### 5.1 Why token-per-minute?

LLM providers rate-limit on tokens, not requests. A 4k-token request
and a 200k-token request count very differently. The scheduler
estimates the per-model TPM from the `usage` table (rolling 60s
window) and throttles.

---

## 6. Visual Rendering

The plan is rendered in the UI as a **React Flow** graph
(see [UI_GUIDELINES §16](./UI_GUIDELINES.md)).

| Visual element         | Meaning                                  |
|------------------------|------------------------------------------|
| Node (rectangle)       | Task; color = role; border = status      |
| Solid edge             | Hard dependency                          |
| Dashed edge            | Soft dependency                          |
| Pulsing node           | Currently running                        |
| Green node             | Approved                                 |
| Red node               | Failed                                   |
| Amber node             | Awaiting review or in repair             |

**Interactions:**

* Click node → opens that task in the right panel
* Drag node → re-order within a level (purely cosmetic; doesn't
  affect scheduling)
* Right-click node → "Mark as failed" (user override)

---

## 7. Repair Sub-graphs

When a task enters `REPAIRING`, the runtime computes a
**re-dispatch set** = { repaired task } ∪ { transitive descendants }.

| Scenario                          | Behavior                                  |
|-----------------------------------|-------------------------------------------|
| Repair succeeds (verdict = PASS)  | Descendants re-dispatched if `PENDING`    |
| Repair fails                      | Whole plan marked `REWRITE_REQUESTED`;   |
|                                   | Chief drafts a partial replan             |
| Repair budget exhausted (3x)      | Workflow → `FAILED`; user prompted        |

---

## 8. Graph Diffing (v0.3)

When a plan is `REWRITE`d, the runtime computes a diff between the
old and new graphs and reports:

* `added` — new tasks
* `removed` — abandoned tasks
* `modified` — title / deliverables / constraints changed
* `unchanged` — preserved as-is

Only the affected sub-graph is re-dispatched.

---

## 9. Plan Templates (v0.2)

A small library of pre-vetted plan templates:

* `feature-crud.md` — backend + frontend + tests
* `bugfix.md` — repro + fix + test
* `refactor.md` — small / medium / large variants
* `greenfield-app.md` — scaffolding + auth + deploy

The Chief picks a template as a starting point and edits it. v0.1
ships **no** templates; plans are written from scratch.

---

## 10. Acceptance Criteria

Each plan has 0+ `AcceptanceCriterion` entries. Each criterion is:

```yaml
- id: ac-1
  description: POST /login returns 200 + JWT for valid creds
  test: cargo test --test login
  automated: true
- id: ac-2
  description: All UI strings externalized
  test: manual
  automated: false
```

* Automated criteria run as part of the workflow.
* Manual criteria appear in the final delivery summary as a
  checklist the user can tick off.

---

## 11. Failure Modes

| Failure                              | Scheduler behavior                  |
|--------------------------------------|-------------------------------------|
| Worker crash mid-task                | Heartbeat timeout → ABORT that task |
| Plan parse error                     | Plan rejected, Chief revises        |
| Cycle in graph                       | Plan rejected, Chief revises        |
| Quota exceeded (TPM/RPM)             | Hold dispatch until window clears   |
| Provider returns malformed JSON      | Task retries once, then fails       |
| `max_parallel_workers` reached       | Tasks wait in PENDING                |

---

## 12. Open Questions

1. Should we support **conditional edges** ("run B only if A's output
   contains 'needs UI'")? (proposed: not in v0.1; v0.3)
2. Should the plan doc be **bidirectional** (Chief edits through the
   graph UI, not just Markdown)? (proposed: yes, in v0.3 with
   `ARCHITECTURE.md` plan visualization)
3. Should `Soft` edges be **inferred** from deliverable analysis
   instead of declared? (proposed: declared in v0.1, inferred in v0.4)

---

**RFC ends.**
