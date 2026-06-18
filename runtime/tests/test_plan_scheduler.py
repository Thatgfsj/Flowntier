"""Tests for `workflow.plan_scheduler`. Phase 2.3."""
from __future__ import annotations

import pytest

from aco_runtime_lib.workflow import (
    TaskNode,
    validate_plan,
)
from aco_runtime_lib.workflow.plan_parser import parse_plan
from aco_runtime_lib.workflow.plan_scheduler import (
    SchedulerOptions,
    TaskStatus,
    compute_repair_subgraph,
    find_ready,
    pick_dispatch,
    reset_to_pending,
)


# ── Helpers ────────────────────────────────────────────────────


def _nodes(*specs: tuple[str, tuple[str, ...], int]) -> list[TaskNode]:
    """Build a list of TaskNode from (id, depends_on, est_tokens)."""
    return [
        TaskNode(
            id=s[0],
            title=s[0],
            owner_role="backend",
            depends_on=s[1],
            est_tokens=s[2],
        )
        for s in specs
    ]


# ── find_ready ────────────────────────────────────────────────


def test_find_ready_empty_no_status() -> None:
    nodes = _nodes(("T1", (), 100))
    statuses: dict[str, TaskStatus] = {}
    assert find_ready(nodes, statuses) == ["T1"]


def test_find_ready_respects_deps() -> None:
    nodes = _nodes(
        ("T1", (), 100),
        ("T2", ("T1",), 200),
        ("T3", ("T2",), 300),
    )
    statuses = {}
    # No deps done → only T1 ready
    assert find_ready(nodes, statuses) == ["T1"]
    # T1 done → T2 ready
    statuses["T1"] = TaskStatus.DONE
    assert find_ready(nodes, statuses) == ["T2"]
    # Both → T3
    statuses["T2"] = TaskStatus.DONE
    assert find_ready(nodes, statuses) == ["T3"]


def test_find_ready_approved_counts_as_dep_done() -> None:
    nodes = _nodes(("T1", (), 100), ("T2", ("T1",), 200))
    assert find_ready(nodes, {"T1": TaskStatus.APPROVED}) == ["T2"]


def test_find_ready_failed_blocks_descendants() -> None:
    """FAILED on a dep does NOT unblock descendants — the parent
    must be repaired first."""
    nodes = _nodes(("T1", (), 100), ("T2", ("T1",), 200))
    ready = find_ready(nodes, {"T1": TaskStatus.FAILED})
    assert ready == []


def test_find_ready_skips_dispatched() -> None:
    nodes = _nodes(("T1", (), 100), ("T2", (), 200))
    statuses = {"T1": TaskStatus.DISPATCHED}
    assert find_ready(nodes, statuses) == ["T2"]


def test_find_ready_fair_short_first() -> None:
    """Within the same topological level, shorter tasks come first."""
    nodes = _nodes(
        ("T1", (), 100),
        ("T2", (), 500),
        ("T3", (), 50),  # shortest
    )
    # Sort key: (depth=0, est_tokens, id) → T3(50) < T1(100) < T2(500)
    assert find_ready(nodes, {}) == ["T3", "T1", "T2"]


# ── pick_dispatch ─────────────────────────────────────────────


def test_pick_dispatch_dispatches_up_to_slots() -> None:
    nodes = _nodes(
        ("T1", (), 100),
        ("T2", (), 100),
        ("T3", (), 100),
    )
    r = pick_dispatch(nodes, {}, SchedulerOptions(max_parallel_workers=2))
    assert set(r.dispatched) == {"T1", "T2"}
    assert "T3" in r.ready


def test_pick_dispatch_no_slots_returns_empty() -> None:
    nodes = _nodes(("T1", (), 100))
    r = pick_dispatch(
        nodes,
        {"T1": TaskStatus.DISPATCHED, "T2": TaskStatus.RUNNING,
         "T3": TaskStatus.RUNNING, "T4": TaskStatus.RUNNING,
         "T5": TaskStatus.RUNNING, "T6": TaskStatus.RUNNING,
         "T7": TaskStatus.RUNNING, "T8": TaskStatus.RUNNING},
        SchedulerOptions(max_parallel_workers=8),
    )
    assert r.dispatched == []


def test_pick_dispatch_active_counted() -> None:
    """2 workers already busy, max=4 → dispatch 2 more."""
    nodes = _nodes(
        ("T1", (), 100), ("T2", (), 100), ("T3", (), 100), ("T4", (), 100),
    )
    statuses = {
        "T5": TaskStatus.RUNNING,
        "T6": TaskStatus.RUNNING,
    }
    r = pick_dispatch(nodes, statuses, SchedulerOptions(max_parallel_workers=4))
    assert len(r.dispatched) == 2


def test_pick_dispatch_finds_blocked() -> None:
    """Blocked = PENDING but not ready."""
    nodes = _nodes(
        ("T1", (), 100),
        ("T2", ("T1",), 100),  # blocked waiting on T1
    )
    r = pick_dispatch(nodes, {})
    assert "T1" in r.dispatched
    # T1 is dispatched; T2 is blocked (depends on T1, not yet done)
    assert "T2" in r.blocked
    assert "T2" not in r.dispatched


def test_pick_dispatch_finds_finished() -> None:
    nodes = _nodes(("T1", (), 100), ("T2", ("T1",), 100))
    r = pick_dispatch(nodes, {"T1": TaskStatus.DONE})
    # T1 in finished (we just dispatched T2; T1 still in statuses)
    assert "T1" in r.finished


# ── Repair sub-graph ──────────────────────────────────────────


def test_repair_subgraph_single() -> None:
    nodes = _nodes(("T1", (), 100), ("T2", ("T1",), 100), ("T3", ("T2",), 100))
    statuses = {"T1": TaskStatus.DONE, "T2": TaskStatus.DONE, "T3": TaskStatus.DONE}
    affected = compute_repair_subgraph("T1", nodes, statuses)
    # All descendants are terminal-success → nothing to re-dispatch
    # except T1 itself (repaired task)
    assert affected == ["T1"]


def test_repair_subgraph_cascade() -> None:
    nodes = _nodes(
        ("T1", (), 100),
        ("T2", ("T1",), 100),
        ("T3", ("T2",), 100),
        ("T4", (), 100),  # unrelated
    )
    statuses = {"T1": TaskStatus.DONE}
    affected = compute_repair_subgraph("T1", nodes, statuses)
    assert set(affected) == {"T1", "T2", "T3"}
    assert "T4" not in affected


def test_repair_subgraph_diamond() -> None:
    """T1 → T2, T1 → T3, T2 → T4, T3 → T4. Repair T1 → all
    descendants affected, but T4 only appears once."""
    nodes = _nodes(
        ("T1", (), 100),
        ("T2", ("T1",), 100),
        ("T3", ("T1",), 100),
        ("T4", ("T2", "T3"), 100),
    )
    affected = compute_repair_subgraph("T1", nodes, {})
    assert set(affected) == {"T1", "T2", "T3", "T4"}


def test_repair_subgraph_keeps_terminal_success() -> None:
    """T2 already APPROVED → not reset by T1's repair."""
    nodes = _nodes(
        ("T1", (), 100),
        ("T2", ("T1",), 100),
    )
    statuses = {"T2": TaskStatus.APPROVED}
    affected = compute_repair_subgraph("T1", nodes, statuses)
    # compute_repair_subgraph only includes T2 if it would need
    # re-running — APPROVED means skip
    assert "T2" not in affected


def test_reset_to_pending_skips_terminal() -> None:
    statuses = {
        "T1": TaskStatus.DONE,
        "T2": TaskStatus.FAILED,
        "T3": TaskStatus.RUNNING,
        "T4": TaskStatus.PENDING,
    }
    reset_to_pending(["T1", "T2", "T3", "T4"], statuses)
    # T1, T2 stay (terminal); T3, T4 reset
    assert statuses["T1"] == TaskStatus.DONE
    assert statuses["T2"] == TaskStatus.FAILED
    assert statuses["T3"] == TaskStatus.PENDING
    assert statuses["T4"] == TaskStatus.PENDING


# ── Integration: parse → validate → schedule ─────────────────


def test_end_to_end_minimax_avatar_fixture() -> None:
    """Parse the avatar fixture, validate, then walk a scheduler
    through the plan to completion. With max_parallel=8 the
    critical path is ~5 levels deep."""
    md = """\
# Plan: User Avatar Upload Endpoint
## Goal
Add avatar endpoint.

## Task Graph
| ID | Title | Owner Role | Depends On | Est. Tokens |
|----|-------|------------|------------|-------------|
| T1 | Design API | Backend | — | 800 |
| T2 | Endpoint skeleton | Backend | T1 | 1000 |
| T3 | MIME validation | Backend | T2 | 600 |
| T4 | Image resize | Backend | T2 | 1200 |
| T5 | S3 upload | Backend | T4 | 1500 |
| T6 | Update DB | Backend | T5 | 500 |
| T7 | Integration tests | QA | T3,T5,T6 | 1800 |
| T8 | Deploy | DevOps | T7 | 500 |

## Acceptance Criteria
1. ok
## Risks
- **r**: d. Mitigated by m.
## Out of Scope
- x
"""
    p = parse_plan(md)
    validate_plan(p.nodes, p.edges)

    statuses: dict[str, TaskStatus] = {}
    opts = SchedulerOptions(max_parallel_workers=8)
    rounds: list[list[str]] = []
    while True:
        r = pick_dispatch(p.nodes, statuses, opts)
        if not r.dispatched:
            break
        rounds.append(list(r.dispatched))
        for tid in r.dispatched:
            statuses[tid] = TaskStatus.DONE
    # Critical path: T1 → T2 → {T3,T4} → T5 → T6 → T7 → T8
    # Max parallelism within critical path is 2 (T3 + T4)
    # So we expect ~5 rounds
    assert len(rounds) >= 4
    assert rounds[0] == ["T1"]
    assert rounds[-1] == ["T8"]
    # All tasks ended up DONE
    assert all(
        statuses[t.id] == TaskStatus.DONE for t in p.nodes
    )