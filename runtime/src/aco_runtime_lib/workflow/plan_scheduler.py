"""Plan scheduler. Phase 2.3.

Decides which tasks are ready and which should be dispatched next
based on the parsed+validated plan and the runtime's worker pool.

Spec: `docs/TASK_GRAPH.md` §4.

Definitions
-----------
* **Ready** = PENDING AND every depends_on is in a terminal-success
  state (DONE or APPROVED).
* **Runnable** = Ready AND `active_workers < max_parallel_workers`.

Algorithm (per `tick`)
----------------------
1. Update task statuses from in-flight results.
2. Find ready tasks, sorted by (topo level, est_tokens ascending).
3. Dispatch up to `(max_parallel_workers - active)` tasks; mark
   them DISPATCHED.

Properties
----------
* **Deterministic** — given the same plan + status, the same
  dispatch order.
* **Fair** — shorter tasks run first within a topological level.
* **Bounded** — never dispatches more than `max_parallel_workers`.

Repair sub-graph
----------------
When a `REPAIR_REQUEST` is sent for task T, the affected set is
T + transitive descendants that haven't reached a terminal state.
`compute_repair_subgraph()` returns that set.

Scope (2.3)
-----------
* Ready/runnable + tick + dispatch decision.
* Repair sub-graph computation.
* The actual worker invocation is owned by `WorkflowOrchestrator`
  (Phase 1). This module is **pure**: it returns lists and decisions,
  not I/O.

Not in 2.3
----------
* TPM/RPM throttling (Phase 2.11/2.12).
* Multi-model health monitoring (Phase 2.12).
* Worker pool implementation (Phase 1, owned by orchestrator).
"""
from __future__ import annotations

from collections.abc import Iterable, Sequence
from dataclasses import dataclass, field
from enum import StrEnum

from aco_runtime_lib.workflow.plan_parser import TaskNode


# ── Task status (mirror of TASK_GRAPH §6) ────────────────────


class TaskStatus(StrEnum):
    PENDING = "PENDING"
    DISPATCHED = "DISPATCHED"
    RUNNING = "RUNNING"
    DONE = "DONE"
    APPROVED = "APPROVED"
    FAILED = "FAILED"
    REPAIRING = "REPAIRING"
    AWAITING_REVIEW = "AWAITING_REVIEW"


TERMINAL_SUCCESS: frozenset[TaskStatus] = frozenset(
    {TaskStatus.DONE, TaskStatus.APPROVED}
)
TERMINAL: frozenset[TaskStatus] = TERMINAL_SUCCESS | frozenset(
    {TaskStatus.FAILED}
)


# ── Options & result ───────────────────────────────────────────


@dataclass(frozen=True)
class SchedulerOptions:
    max_parallel_workers: int = 8


@dataclass
class TickResult:
    """The decision of one tick."""
    dispatched: list[str] = field(default_factory=list)
    ready: list[str] = field(default_factory=list)
    blocked: list[str] = field(default_factory=list)
    finished: list[str] = field(default_factory=list)


# ── Core ───────────────────────────────────────────────────────


def find_ready(
    nodes: Sequence[TaskNode],
    statuses: dict[str, TaskStatus],
) -> list[str]:
    """Return task IDs whose status is PENDING and every depends_on
    is in TERMINAL_SUCCESS. Order: topo level first (nodes with
    fewer deps-first), then by est_tokens ascending."""
    ready: list[str] = []
    by_id = {n.id: n for n in nodes}
    for n in nodes:
        if statuses.get(n.id, TaskStatus.PENDING) != TaskStatus.PENDING:
            continue
        if all(
            statuses.get(dep) in TERMINAL_SUCCESS
            for dep in n.depends_on
        ):
            ready.append(n.id)
    # Sort by (depth = max topo depth of deps, est_tokens)
    depth: dict[str, int] = {n.id: 0 for n in nodes}
    for n in nodes:
        for d in n.depends_on:
            depth[n.id] = max(depth[n.id], depth.get(d, 0) + 1)
    ready.sort(
        key=lambda tid: (depth[tid], by_id[tid].est_tokens, tid)
    )
    return ready


def pick_dispatch(
    nodes: Sequence[TaskNode],
    statuses: dict[str, TaskStatus],
    options: SchedulerOptions | None = None,
) -> TickResult:
    """One scheduler tick: returns the list of task IDs to dispatch.

    The caller (orchestrator) is responsible for actually invoking
    the worker and updating `statuses[tid] = DISPATCHED`.

    Bound: never dispatches more than `max_parallel_workers -
    active_workers`. `active_workers` is computed from `statuses`
    as the count of DISPATCHED + RUNNING tasks.
    """
    opts = options or SchedulerOptions()
    active = sum(
        1 for s in statuses.values()
        if s in (TaskStatus.DISPATCHED, TaskStatus.RUNNING)
    )
    slots = max(0, opts.max_parallel_workers - active)

    ready = find_ready(nodes, statuses)
    blocked = [
        n.id for n in nodes
        if statuses.get(n.id, TaskStatus.PENDING) == TaskStatus.PENDING
        and n.id not in ready
    ]
    finished = [tid for tid, s in statuses.items() if s in TERMINAL]

    if slots == 0:
        return TickResult(dispatched=[], ready=ready, blocked=blocked,
                          finished=finished)

    dispatched = ready[:slots]
    return TickResult(
        dispatched=dispatched,
        ready=ready,
        blocked=blocked,
        finished=finished,
    )


# ── Repair sub-graph ─────────────────────────────────────────


def compute_repair_subgraph(
    repaired_task: str,
    nodes: Sequence[TaskNode],
    statuses: dict[str, TaskStatus],
) -> list[str]:
    """Return the set of tasks to re-dispatch after a REPAIR on
    `repaired_task`: the task itself + every transitive descendant
    that hasn't reached a terminal-success state.

    Used by the orchestrator when a task verdict is REPAIR:
    1. mark `repaired_task` → REPAIRING
    2. reset each affected descendant → PENDING (if not already
       terminal-success)
    3. scheduler picks them up on the next tick
    """
    successors: dict[str, list[str]] = {n.id: [] for n in nodes}
    for n in nodes:
        for dep in n.depends_on:
            successors[dep].append(n.id)

    affected: set[str] = {repaired_task}
    frontier = [repaired_task]
    while frontier:
        current = frontier.pop()
        for succ in successors[current]:
            if succ in affected:
                continue
            # Don't bring back tasks that already finished
            if statuses.get(succ) in TERMINAL_SUCCESS:
                continue
            affected.add(succ)
            frontier.append(succ)
    # Stable order: by task id
    return sorted(affected)


def reset_to_pending(
    tasks: Iterable[str],
    statuses: dict[str, TaskStatus],
) -> None:
    """Mutate `statuses` so each task in `tasks` is PENDING. Tasks
    already in a terminal state are left alone (defensive)."""
    for tid in tasks:
        if statuses.get(tid) in TERMINAL:
            continue
        statuses[tid] = TaskStatus.PENDING