"""Workflow state machine.

Implements the 19-state machine from `docs/WORKFLOW_SPEC.md` §3-§4.
States are an `enum.StrEnum` so they serialize as their string name
(matches the Rust `&str` constants and the SQLite rows).

Transitions are a frozen table of `(from, event, to)` tuples plus a
guard callable. The guard receives the current `WorkflowCtx` and
returns `True` if the transition is allowed.

The machine is **synchronous** in the sense that `transition()` is a
single atomic step from the caller's perspective. It emits a
`WfEvent.transition` on the bus before returning, and (in Phase 1)
appends to the JSONL log.

Terminal states: `DONE`, `FAILED`, `ABORTED`. The machine refuses
all transitions out of a terminal state.
"""

from __future__ import annotations

import enum
from collections.abc import Callable
from dataclasses import dataclass, field
from typing import Any, Final

from aco_runtime_lib.event_bus import EventBus, WfEvent


class State(enum.StrEnum):
    """The 19 workflow states."""

    # Phase 1 — Requirement
    REQ_RECEIVED = "REQ_RECEIVED"
    REQ_ANALYZING = "REQ_ANALYZING"
    REQ_AWAIT_USER = "REQ_AWAIT_USER"
    REQ_CLARIFIED = "REQ_CLARIFIED"

    # Phase 2 — Planning
    PLAN_DRAFTING = "PLAN_DRAFTING"
    PLAN_DRAFTED = "PLAN_DRAFTED"

    # Phase 3 — Plan Review
    PLAN_UNDER_REVIEW = "PLAN_UNDER_REVIEW"
    PLAN_REVISING = "PLAN_REVISING"
    PLAN_APPROVED = "PLAN_APPROVED"

    # Phase 4 — Dispatch
    DISPATCHING = "DISPATCHING"

    # Phase 5 — Development
    DEVELOPING = "DEVELOPING"
    AWAITING_WORKERS = "AWAITING_WORKERS"

    # Phase 6 — Review
    REVIEWING = "REVIEWING"

    # Phase 7 — Repair
    REPAIRING = "REPAIRING"
    REWRITING = "REWRITING"

    # Phase 8 — Delivery
    DELIVERING = "DELIVERING"
    FINAL_REVIEW = "FINAL_REVIEW"
    DONE = "DONE"

    # Terminal — failure paths
    FAILED = "FAILED"
    ABORTED = "ABORTED"


TERMINAL_STATES: Final[frozenset[State]] = frozenset({State.DONE, State.FAILED, State.ABORTED})


@dataclass(frozen=True, slots=True)
class Transition:
    """One allowed transition: `(from, event) -> to`."""

    from_state: State
    event: str
    to_state: State
    guard: Callable[[WorkflowCtx], bool] | None = None


@dataclass(slots=True)
class WorkflowCtx:
    """Mutable context shared with guards and event listeners.

    The `data` dict is a free-form scratchpad for chief / critics /
    workers to communicate through the state machine.
    """

    wf_id: str
    actor: str
    data: dict[str, Any] = field(default_factory=dict)
    repair_count: int = 0
    plan_revision_count: int = 0


# ── Guards ───────────────────────────────────────────────────────


def _has_open_questions(ctx: WorkflowCtx) -> bool:
    return len(ctx.data.get("open_questions", [])) > 0


def _no_open_questions(ctx: WorkflowCtx) -> bool:
    return not _has_open_questions(ctx)


def _both_critics_done(ctx: WorkflowCtx) -> bool:
    critic_a = ctx.data.get("critic_a_verdict")
    critic_b = ctx.data.get("critic_b_verdict")
    return critic_a is not None and critic_b is not None


def _critics_raised_issues(ctx: WorkflowCtx) -> bool:
    return bool(ctx.data.get("any_major_issue", False))


def _under_repair_budget(ctx: WorkflowCtx) -> bool:
    return ctx.repair_count < 3


def _plan_revision_budget_exhausted(ctx: WorkflowCtx) -> bool:
    """Guard for the `max_revisions` transition.

    The caller fires `max_revisions` when they have hit the budget
    cap. The transition is rejected (InvalidTransitionError) if
    the counter is below the cap, because firing it early would
    skip revisions that should still be allowed.
    """
    return ctx.plan_revision_count >= 3


def _plan_verdict_allows_approval(ctx: WorkflowCtx) -> bool:
    """Approve the plan only if the Chief has set the verdict AND
    there are no major issues. The Chief writes `ctx.data["plan_verdict"]`
    = "APPROVED" | "REVISING" before firing `both_critics_done`.
    """
    return ctx.data.get("plan_verdict") == "APPROVED"


def _plan_verdict_requires_revision(ctx: WorkflowCtx) -> bool:
    return ctx.data.get("plan_verdict") == "REVISING"


# ── Transition table ─────────────────────────────────────────────
# Mirrors `docs/WORKFLOW_SPEC.md` §4 exactly.

TRANSITIONS: Final[tuple[Transition, ...]] = (
    # Phase 1
    Transition(State.REQ_RECEIVED, "start_analysis", State.REQ_ANALYZING),
    Transition(
        State.REQ_ANALYZING, "need_clarification", State.REQ_AWAIT_USER, _has_open_questions
    ),
    Transition(State.REQ_ANALYZING, "analysis_done", State.REQ_CLARIFIED, _no_open_questions),
    Transition(State.REQ_AWAIT_USER, "user_responded", State.REQ_ANALYZING),
    Transition(State.REQ_AWAIT_USER, "user_timeout", State.FAILED),
    # Phase 2
    Transition(State.REQ_CLARIFIED, "start_planning", State.PLAN_DRAFTING),
    Transition(State.PLAN_DRAFTING, "plan_emitted", State.PLAN_DRAFTED),
    # Phase 3
    Transition(State.PLAN_DRAFTED, "dispatch_review", State.PLAN_UNDER_REVIEW),
    # The Chief decides APPROVED vs REVISING (based on critic output)
    # and writes `ctx.data["plan_verdict"]` before firing the event.
    # Guards are mutually exclusive; iteration order is therefore
    # unambiguous.
    Transition(
        State.PLAN_UNDER_REVIEW,
        "both_critics_done",
        State.PLAN_REVISING,
        _plan_verdict_requires_revision,
    ),
    Transition(
        State.PLAN_UNDER_REVIEW,
        "both_critics_done",
        State.PLAN_APPROVED,
        _plan_verdict_allows_approval,
    ),
    Transition(State.PLAN_REVISING, "plan_revised", State.PLAN_DRAFTED),
    Transition(
        State.PLAN_REVISING,
        "max_revisions",
        State.FAILED,
        _plan_revision_budget_exhausted,
    ),
    # Phase 4
    Transition(State.PLAN_APPROVED, "start_dispatch", State.DISPATCHING),
    # Phase 5
    Transition(State.DISPATCHING, "all_assigned", State.AWAITING_WORKERS),
    Transition(State.AWAITING_WORKERS, "first_result", State.DEVELOPING),
    Transition(State.DEVELOPING, "all_results_in", State.REVIEWING),
    Transition(State.DEVELOPING, "task_ask", State.AWAITING_WORKERS),
    # Phase 6
    Transition(State.REVIEWING, "verdict_pass", State.DELIVERING),
    Transition(State.REVIEWING, "verdict_repair", State.REPAIRING, _under_repair_budget),
    Transition(State.REVIEWING, "verdict_rewrite", State.REWRITING),
    # Phase 7
    Transition(State.REPAIRING, "all_repaired", State.REVIEWING),
    Transition(State.REPAIRING, "budget_exceeded", State.FAILED),
    Transition(State.REWRITING, "replan_done", State.PLAN_REVISING),
    # Phase 8
    Transition(State.DELIVERING, "report_emitted", State.FINAL_REVIEW),
    # The "started" event is fired for symmetry with other phases
    # so observers (UI, logs) can see the review begin.
    Transition(State.DELIVERING, "final_review_started", State.FINAL_REVIEW),
    # Final review after the delivery summary is drafted. PASS
    # -> DONE. REPAIR -> re-loop through the worker + repair
    # cycle. REJECT -> FAILED (the whole workflow missed the
    # point; retrying won't help).
    Transition(
        State.FINAL_REVIEW,
        "final_review_pass",
        State.DONE,
    ),
    Transition(
        State.FINAL_REVIEW,
        "final_review_repair",
        State.REPAIRING,
        _under_repair_budget,
    ),
    Transition(
        State.FINAL_REVIEW,
        "final_review_reject",
        State.FAILED,
    ),
    # Universal: any state can be aborted
    Transition(State.REQ_RECEIVED, "user_abort", State.ABORTED),
    Transition(State.REQ_ANALYZING, "user_abort", State.ABORTED),
    Transition(State.PLAN_DRAFTING, "user_abort", State.ABORTED),
    Transition(State.DEVELOPING, "user_abort", State.ABORTED),
    Transition(State.REVIEWING, "user_abort", State.ABORTED),
    Transition(State.DELIVERING, "user_abort", State.ABORTED),
)


class InvalidTransitionError(Exception):
    """Raised when a transition is not allowed from the current state."""


class StateMachine:
    """Workflow state machine. Not thread-safe; intended to be owned
    by a single async task per workflow run.
    """

    def __init__(
        self, ctx: WorkflowCtx, bus: EventBus, *, initial: State = State.REQ_RECEIVED
    ) -> None:
        self._ctx = ctx
        self._bus = bus
        self._state: State = initial

    @property
    def state(self) -> State:
        return self._state

    @property
    def ctx(self) -> WorkflowCtx:
        return self._ctx

    @property
    def is_terminal(self) -> bool:
        return self._state in TERMINAL_STATES

    async def transition(self, event: str) -> State:
        """Fire `event`; return the new state.

        Raises `InvalidTransitionError` if no transition matches
        (wrong state, wrong event, or guard returned False).
        """
        if self.is_terminal:
            raise InvalidTransitionError(f"cannot fire {event!r}: state {self._state} is terminal")

        for t in TRANSITIONS:
            if t.from_state != self._state or t.event != event:
                continue
            if t.guard is not None and not t.guard(self._ctx):
                continue
            from_state = self._state
            self._state = t.to_state
            # Update side counters the runtime cares about.
            if event == "verdict_repair":
                self._ctx.repair_count += 1
            if event == "plan_revised":
                self._ctx.plan_revision_count += 1
            await self._bus.publish(
                WfEvent.transition(
                    wf_id=self._ctx.wf_id,
                    from_state=from_state.value,
                    to_state=t.to_state.value,
                    event=event,
                    actor=self._ctx.actor,
                )
            )
            return self._state

        raise InvalidTransitionError(f"no transition matches state={self._state} event={event!r}")
