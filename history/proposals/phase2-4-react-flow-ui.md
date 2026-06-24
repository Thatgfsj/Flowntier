# RFC: Plan Visualization UI (Phase 2.4)

> React Flow graph for the Chief's plan, behind a tab in Z3.

**Status:** Proposed
**Author:** Thatgfsj
**Date:** 2026-06-19
**Phase:** 2.4 (5 days)
**Related:** [TASK_GRAPH.md §6](../TASK_GRAPH.md) (data shape) ·
[UI_GUIDELINES.md §1](../UI_GUIDELINES.md) (Mission Control principles) ·
[phase2-1-plan-parser.md](./phase2-1-plan-parser.md) (parser that
produces the AST this RFC consumes)

---

## 1. Problem

TASK_GRAPH.md §6 says the plan is rendered as a **React Flow**
graph: role-colored nodes, status-colored borders, click/drag,
right-click "mark as failed". But the parser (Phase 2.1) only
shipped last session, and the UI has no plan viewer yet — the
right-panel shows a task list, not a graph.

There is also a **design tension** with `UI_GUIDELINES.md §1`,
which lists "complex node-and-edge graphs" as an explicit
**anti-pattern**. The Mission Control philosophy favors the
**timeline + event stream** as the primary surface, with graphs
serving a secondary role.

This RFC resolves that tension and specifies the minimum UI
needed to ship Phase 2.4 on schedule.

---

## 2. Resolution of the Mission-Control vs. Graph tension

Three options considered:

1. **Replace the right-panel task list with the graph** —
   violates UI_GUIDELINES §1 ("complex graphs as primary
   surface"). Rejected.
2. **Show the graph on top of the timeline always** —
   replaces the primary surface. Rejected.
3. **Graph lives behind a tab in Z3 (CENTER)** — the user
   flips to "Plan" view only when they want to inspect or edit.
   The default tab stays "Discussion" (event stream + Chief
   reasoning). **Chosen.**

Z3 CENTER is the multi-mode content area (Discussion / Reasoning /
Review / Task / **Plan**). The user is already trained to flip
tabs here, so the graph slot is a natural fit.

---

## 3. Goals & Non-goals

**Goals**

1. Render `ParsedPlan.nodes + ParsedPlan.edges` as a React Flow
   graph inside the new Z3 "Plan" tab.
2. Visual encoding per `TASK_GRAPH §6`:
   * Node fill color = owner role
   * Node border = status (DONE/APPROVED/FAILED/REPAIRING/…)
   * Pulsing animation = currently running
   * Solid edge = Hard dependency
   * Dashed edge = Soft dependency (deferred; see §6)
3. Click node → opens a task-detail panel (Z4 RIGHT scroll).
4. Drag node → re-order within a topological level (cosmetic;
   no scheduling effect). Documented but doesn't mutate the AST.
5. Right-click node → "Mark as failed" (user override).
6. Live updates: graph re-renders when the orchestrator emits
   a `WfEvent.task_status` for a node in the current plan.

**Non-goals** (out of scope for 2.4)

* Soft edges (TASK_GRAPH §6 mentions them; the parser/validator
  never emit them — tracked in phase2-1 RFC §10.3).
* Conditional edges ("run B only if A's output contains X",
  TASK_GRAPH §12). Deferred to v0.3.
* Plan diffing (TASK_GRAPH §8). Deferred to v0.3.
* Auto-layout algorithms (we use a static topo-level layout
  per §4.2). User-driven re-arrangement only.

---

## 4. UI spec

### 4.1 New tab in Z3 CENTER

```
Z3 CENTER tab strip:
  [ Discussion ] [ Reasoning ] [ Review ] [ Plan ] [ Console ]
                                     ▲
                                     new in 2.4
```

Tab badge shows `parse_error` / `validation_error` count
(surface the repair-loop feedback directly in the UI).

### 4.2 Layout algorithm

Reactive Flow's `dagre` layout, **left-to-right**, with these
inputs:

* nodes: `ParsedPlan.nodes` (each with `id`, `title`,
  `owner_role`, `est_tokens`)
* edges: `ParsedPlan.edges` (only Hard in 2.4)

Algorithm:

1. Group nodes by **topological depth** (longest path from any
   root). Reuses the topo order from
   `validate_plan(...).topo_order`.
2. Within a depth level, sort by `est_tokens` ascending (so
   short tasks sit on the left, matching the scheduler's
   fairness rule).
3. Dagre lays out level-by-level, left → right.
4. Edge style: solid 2px (`Hard`).

### 4.3 Visual encoding

| Element            | Encoding                                |
|--------------------|------------------------------------------|
| Node rectangle     | 180×60, rounded 8px                     |
| Fill color         | Owner role palette (8 colors, see §5)  |
| Border (default)   | 1px solid `border`                     |
| Border (running)   | 2px solid `chief`, pulsing 1.5s        |
| Border (done)      | 2px solid `success`                    |
| Border (failed)    | 2px solid `danger`                     |
| Border (approved)  | 2px solid `success`                     |
| Border (repairing) | 2px dashed `warning`                    |
| Title              | First 24 chars of `task.title`         |
| Subtitle           | `owner_role` · `est_tokens`             |
| Edge               | Solid 1.5px `border-strong` (Hard)     |

Dark theme: all colors picked from the existing Tailwind palette
in `apps/desktop/src/index.css`.

### 4.4 Interactions

| User action              | Effect                              |
|--------------------------|--------------------------------------|
| Click node               | Open task detail in Z4 RIGHT scroll |
| Drag node (within level) | Re-order; cosmetic only            |
| Right-click node         | Context menu: "Mark as failed", "View source" (latter stubs in 2.4) |
| Zoom/pan canvas          | Standard React Flow controls         |
| Fit-to-view button       | Reset viewport to default           |
| Hover edge               | Tooltip: `T3 → T5 (Hard)`          |

---

## 5. Owner-role palette

| Role         | Tailwind token | Hex (dark theme) |
|--------------|----------------|-------------------|
| Backend      | `accent-blue`  | `#3b82f6`         |
| Frontend     | `accent-cyan`  | `#06b6d4`         |
| Database     | `accent-violet`| `#8b5cf6`         |
| DevOps       | `accent-orange`| `#f97316`         |
| QA           | `accent-green` | `#22c55e`         |
| Docs         | `accent-amber` | `#f59e0b`         |
| Security     | `accent-red`   | `#ef4444`         |
| Other        | `text-secondary`| `#9ca3af`        |

If the role isn't in the table, fall back to `text-secondary`.

---

## 6. Data flow

```
Chief's plan_md
   ↓ parse_plan()       → ParsedPlan (already shipped)
   ↓ POST /api/workflow/{id}/plan  → runtime stores it
   ↓ GET /api/workflow/{id}/plan   → returns ParsedPlan JSON
   ↓ React Flow renders
```

A new endpoint `GET /api/workflow/{id}/plan` returns the
`ParsedPlan` for the running workflow. The UI polls this
endpoint every 2s while the workflow is active (same cadence as
the existing `/api/workflow/{id}/summary` poll).

The same `currentWfId` in `window.__acoCurrentWfId` (added in
commit `a141f08`) drives the poll.

### 6.1 Status updates

For status borders (running/done/failed/…), the plan graph
**derives** them from the existing `WfEvent.task_status`
WebSocket events. No new endpoints needed — the events already
carry `task_id` and `status` (per `crates/event-bus` §10).

### 6.2 User override: "Mark as failed"

Sends `POST /api/workflow/{id}/task/{task_id}/fail` (new
endpoint). The runtime moves the task to FAILED and re-evaluates
the ready set on the next tick. The graph updates immediately
via the WS event.

---

## 7. New runtime endpoint

`GET /api/workflow/{id}/plan` returns:

```json
{
  "id": "wf_abc",
  "parsed_plan": { /* ParsedPlan */ },
  "topo_order": ["T1", "T2", "T3", "T4"],
  "parse_error": null,
  "validation_error": null
}
```

`POST /api/workflow/{id}/task/{task_id}/fail` accepts a body
`{"reason": "..."}` and returns 200 with the updated task
status, or 409 if the workflow is terminal.

Both are small (~50 LOC each) and unblock the UI.

---

## 8. Implementation order

1. **Endpoint** `GET /plan` + `POST /task/{id}/fail`
   (1 day, backend)
2. **React Flow component** `PlanGraph.tsx` with the dagre
   layout (1.5 days, frontend)
3. **Tab wiring** in Z3 CENTER (0.5 day, frontend)
4. **Status updates** from WfEvent.task_status WS (0.5 day)
5. **Tests + Playwright smoke** (1 day)
6. **Polish** (color tuning, edge hover, fit-to-view) (0.5 day)

Total: ~5 days.

---

## 9. Test matrix

`apps/desktop/tests/plan-graph.test.tsx` (vitest + testing-library):

| Test                                                | What it checks |
|-----------------------------------------------------|-----------------|
| `renders all nodes from parsed_plan`                | 8 tasks → 8 nodes |
| `applies owner_role fill color`                     | Backend → blue   |
| `shows pulsing border on RUNNING status`            | Animation class applied |
| `shows green border on DONE status`                 | success border  |
| `click node fires onTaskSelect callback`            | Z4 panel scrolls |
| `drag within level updates order`                   | Cosmetic; re-renders |
| `shows parse_error badge when parse_error is set`   | Red dot on tab |
| `shows validation_error badge on cycle`             | Red dot on tab |
| `soft edges rendered dashed (when present)`         | Future-proofing  |

Plus a Playwright e2e (`scripts/snap_plan_graph.js`) that
captures a screenshot of the Plan tab mid-workflow.

---

## 10. Acceptance criteria

1. `GET /api/workflow/{id}/plan` returns a valid `ParsedPlan`
   for any completed workflow.
2. The Plan tab renders all `ParsedPlan.nodes` as React Flow
   nodes, with the right colors and status borders.
3. Clicking a node scrolls Z4 RIGHT to the task detail.
4. Status updates via WS re-render the border within 1s.
5. "Mark as failed" right-click moves the task to FAILED and
   the runtime re-evaluates descendants.
6. The Mission-Control default surface (Discussion / Timeline)
   is **not** affected; the Plan tab is opt-in.
7. `<PlanGraph />` is a presentational component — given a
   `ParsedPlan` + a status map, it renders correctly with no
   I/O. Unit-testable in isolation.
8. vitest + Playwright snapshots both pass.

---

## 11. Risks

| Risk                                       | Mitigation                                      |
|--------------------------------------------|--------------------------------------------------|
| React Flow bundle size                     | Code-split via lazy import in the tab route     |
| Dagre perf with 100 nodes                 | Tested with the minimax-avatar fixture (12 nodes); OK |
| Live re-render thrash on busy workflows   | Throttle status updates to 1Hz in the graph layer |
| User drag mutates AST unexpectedly        | Drag is cosmetic only — does NOT mutate `parsed_plan`; documented |

---

## 12. Open questions

1. **Tab badge granularity** — one badge for parse+validation
   errors combined, or separate? *Proposed: combined, with
   a tooltip that lists them.*
2. **Soft edges** — never emitted by parser today. *Proposed:
   render as dashed anyway when present in `ParsedPlan.edges`,
   so 2.4 is future-proof.*
3. **Right-click "Mark as failed" undo** — should there be an
   undo? *Proposed: no undo in 2.4; the user can use the
   Chief's REPAIR flow to recover. Tracked.*

---

## 13. Migration / Rollout

1. Backend: add `GET /plan` and `POST /task/{id}/fail` endpoints.
2. Frontend: install `reactflow`, add `<PlanGraph />` under
   `apps/desktop/src/zones/PlanGraph.tsx`.
3. Wire the new tab into Z3 CENTER's tab strip.
4. The Chief's repair loop already uses `parse_error` and
   `validation_error` from `PlannerAgent.run`; surface them on
   the tab badge.
5. v0.3 follow-ups: soft edges, plan diff, conditional edges.

---

## 14. Effort & Dependencies

* Estimate: 5 days (per `plans/Phase2.md`).
* Blocked by: Phase 2.1 parser (done), Phase 2.2 validator (done),
  Phase 2.3 scheduler (done — provides `topo_order`).
* Blocks: 2.5–2.18 (other Phase 2 items that surface plan data).
* Risk: bundle size and tab discoverability (users might not
  realize the Plan tab exists). Mitigation: small "Plan" hint
  pulse on first workflow.

---

**RFC ends.**