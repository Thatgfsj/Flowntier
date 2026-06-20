"""End-to-end test for the workflow state machine.

This is the **MVP** for Phase 0.1: drives a workflow from
`REQ_RECEIVED` all the way to `DONE` without any real LLM, by
feeding pre-canned Critic verdicts and Worker results.

Validates:
* Every transition in `WORKFLOW_SPEC.md` §4 fires correctly.
* Guards work (open questions, critic verdicts, repair budget).
* The state machine refuses to leave a terminal state.
* Invalid transitions raise `InvalidTransitionError`.
* The event bus receives one `transition` event per fired event.
"""

from __future__ import annotations

import pytest
from aco_runtime_lib import EventBus, State, StateMachine, WfEvent
from aco_runtime_lib.workflow import WorkflowCtx
from aco_runtime_lib.workflow.state_machine import (
    TERMINAL_STATES,
    TRANSITIONS,
    InvalidTransitionError,
)

# ── Helpers ─────────────────────────────────────────────────────


def _make_sm() -> tuple[StateMachine, EventBus]:
    bus = EventBus()
    ctx = WorkflowCtx(wf_id="wf_test", actor="agent:chief")
    return StateMachine(ctx, bus), bus


async def _drive_to_done(sm: StateMachine) -> list[WfEvent]:
    """Run the canonical happy path: REQ_RECEIVED → DONE.

    Feeds the guards whatever they need to advance. Collects every
    event published on the bus, using a wrapper around `transition`
    that drains the queue synchronously after each step (avoids race
    conditions with the event loop).
    """
    events: list[WfEvent] = []
    bus = sm._bus
    queue = await bus.subscribe()

    original_transition = sm.transition

    async def tracked_transition(event: str) -> State:
        new_state = await original_transition(event)
        while not queue.empty():
            events.append(queue.get_nowait())
        return new_state

    sm.transition = tracked_transition  # type: ignore[method-assign]

    try:
        # Phase 1 — Requirement (no open questions, so direct path)
        await sm.transition("start_analysis")
        await sm.transition("analysis_done")

        # Phase 2 — Planning
        await sm.transition("start_planning")
        await sm.transition("plan_emitted")

        # Phase 3 — Plan Review (both critics PASS, no issues)
        await sm.transition("dispatch_review")
        sm.ctx.data["critic_a_verdict"] = "PASS"
        sm.ctx.data["critic_b_verdict"] = "PASS"
        sm.ctx.data["plan_verdict"] = "APPROVED"
        await sm.transition("both_critics_done")
        assert sm.state == State.PLAN_APPROVED

        # Phase 4 — Dispatch
        await sm.transition("start_dispatch")
        await sm.transition("all_assigned")

        # Phase 5 — Development
        await sm.transition("first_result")
        await sm.transition("all_results_in")

        # Phase 6 — Review (verdict PASS)
        await sm.transition("verdict_pass")

        # Phase 8 — Delivery
        await sm.transition("report_emitted")
        # Phase 8b — Final Review (Phase 2 / C2). PASS path.
        await sm.transition("final_review_pass")
        assert sm.state == State.DONE
        assert sm.is_terminal
    finally:
        sm.transition = original_transition  # type: ignore[method-assign]
        await bus.unsubscribe(queue)

    return events


# ── Tests ────────────────────────────────────────────────────────


def test_terminal_states_are_a_frozenset() -> None:
    assert TERMINAL_STATES == frozenset({State.DONE, State.FAILED, State.ABORTED})


def test_transitions_table_covers_all_workflow_spec_states() -> None:
    """Every non-terminal state must have at least one outgoing transition."""
    outgoing: set[State] = {t.from_state for t in TRANSITIONS}
    for s in State:
        if s in TERMINAL_STATES:
            continue
        assert s in outgoing, f"state {s} has no outgoing transitions"


def test_all_transitions_target_known_states() -> None:
    for t in TRANSITIONS:
        assert t.from_state in State, f"unknown from_state: {t.from_state}"
        assert t.to_state in State, f"unknown to_state: {t.to_state}"


@pytest.mark.asyncio
async def test_happy_path_req_to_done() -> None:
    sm, _ = _make_sm()
    events = await _drive_to_done(sm)
    assert sm.state == State.DONE

    transition_events = [e for e in events if e.kind == "transition"]
    assert len(transition_events) == 13
    states = [e.to_state for e in transition_events]
    assert states == [
        "REQ_ANALYZING",
        "REQ_CLARIFIED",
        "PLAN_DRAFTING",
        "PLAN_DRAFTED",
        "PLAN_UNDER_REVIEW",
        "PLAN_APPROVED",
        "DISPATCHING",
        "AWAITING_WORKERS",
        "DEVELOPING",
        "REVIEWING",
        "DELIVERING",
        "FINAL_REVIEW",
        "DONE",
    ]


@pytest.mark.asyncio
async def test_terminal_state_refuses_all_transitions() -> None:
    sm, _ = _make_sm()
    await _drive_to_done(sm)
    assert sm.is_terminal
    with pytest.raises(InvalidTransitionError):
        await sm.transition("user_abort")


@pytest.mark.asyncio
async def test_invalid_transition_raises() -> None:
    sm, _ = _make_sm()
    with pytest.raises(InvalidTransitionError):
        await sm.transition("plan_emitted")  # wrong state


@pytest.mark.asyncio
async def test_open_questions_force_await_user() -> None:
    sm, _ = _make_sm()
    await sm.transition("start_analysis")
    sm.ctx.data["open_questions"] = ["What auth method?"]
    await sm.transition("need_clarification")
    assert sm.state == State.REQ_AWAIT_USER
    await sm.transition("user_responded")
    assert sm.state == State.REQ_ANALYZING


@pytest.mark.asyncio
async def test_critic_issues_send_plan_to_revision() -> None:
    sm, _ = _make_sm()
    await sm.transition("start_analysis")
    await sm.transition("analysis_done")
    await sm.transition("start_planning")
    await sm.transition("plan_emitted")
    await sm.transition("dispatch_review")
    sm.ctx.data["critic_a_verdict"] = "REPAIR"
    sm.ctx.data["critic_b_verdict"] = "PASS"
    sm.ctx.data["plan_verdict"] = "REVISING"
    await sm.transition("both_critics_done")
    assert sm.state == State.PLAN_REVISING


@pytest.mark.asyncio
async def test_repair_loop_increments_counter() -> None:
    sm, _ = _make_sm()
    await sm.transition("start_analysis")
    await sm.transition("analysis_done")
    await sm.transition("start_planning")
    await sm.transition("plan_emitted")
    await sm.transition("dispatch_review")
    sm.ctx.data["critic_a_verdict"] = "REPAIR"
    sm.ctx.data["critic_b_verdict"] = "PASS"
    sm.ctx.data["plan_verdict"] = "REVISING"
    await sm.transition("both_critics_done")
    assert sm.ctx.plan_revision_count == 0
    await sm.transition("plan_revised")
    assert sm.ctx.plan_revision_count == 1
    assert sm.state == State.PLAN_DRAFTED


@pytest.mark.asyncio
async def test_user_abort_works_from_any_phase() -> None:
    sm, _ = _make_sm()
    await sm.transition("user_abort")
    assert sm.state == State.ABORTED
    assert sm.is_terminal


@pytest.mark.asyncio
async def test_events_are_published_to_bus() -> None:
    """Direct bus publish test (no state machine)."""
    sm, bus = _make_sm()
    queue = await bus.subscribe()

    await sm.transition("user_abort")
    # Drain the queue
    received: list[WfEvent] = []
    while not queue.empty():
        received.append(queue.get_nowait())

    assert len(received) == 1
    assert received[0].kind == "transition"
    assert received[0].to_state == "ABORTED"
    assert received[0].event == "user_abort"
    assert received[0].actor == "agent:chief"

    await bus.unsubscribe(queue)


@pytest.mark.asyncio
async def test_repair_budget_exhaustion() -> None:
    """3 plan revisions then FAILED via max_revisions."""
    sm, _ = _make_sm()

    async def one_revision_round() -> None:
        await sm.transition("dispatch_review")
        sm.ctx.data["plan_verdict"] = "REVISING"
        await sm.transition("both_critics_done")
        await sm.transition("plan_revised")

    await sm.transition("start_analysis")
    await sm.transition("analysis_done")
    await sm.transition("start_planning")
    await sm.transition("plan_emitted")
    # Three full revision rounds
    await one_revision_round()
    await one_revision_round()
    await one_revision_round()
    assert sm.ctx.plan_revision_count == 3
    # One more REVISING → still in PLAN_REVISING, but counter not bumped
    await sm.transition("dispatch_review")
    sm.ctx.data["plan_verdict"] = "REVISING"
    await sm.transition("both_critics_done")
    # Now fire max_revisions (budget exhausted)
    await sm.transition("max_revisions")
    assert sm.state == State.FAILED
    assert sm.is_terminal


@pytest.mark.asyncio
async def test_final_review_reject_from_repairing() -> None:
    """The FinalReviewer verdict emitted after a repair cycle must
    be accepted from REPAIRING. v0.2.2 was missing these transitions
    and crashed with InvalidTransitionError on REJECT.
    Found by dogfooding capture on 2026-06-19."""
    sm, _ = _make_sm()
    # Walk to REPAIRING
    await sm.transition("start_analysis")
    await sm.transition("analysis_done")
    await sm.transition("start_planning")
    await sm.transition("plan_emitted")
    await sm.transition("dispatch_review")
    sm.ctx.data["plan_verdict"] = "APPROVED"
    await sm.transition("both_critics_done")
    await sm.transition("start_dispatch")
    await sm.transition("all_assigned")
    await sm.transition("first_result")
    await sm.transition("all_results_in")
    await sm.transition("verdict_repair")
    assert sm.state == State.REPAIRING
    # FinalReviewer emits REJECT from REPAIRING. This is the path
    # that crashed in production on 2026-06-19.
    await sm.transition("final_review_reject")
    assert sm.state == State.FAILED
    assert sm.is_terminal


@pytest.mark.asyncio
async def test_final_review_pass_from_repairing() -> None:
    """The FinalReviewer can also emit PASS from REPAIRING (a happy
    path where repair cycle succeeds)."""
    sm, _ = _make_sm()
    await sm.transition("start_analysis")
    await sm.transition("analysis_done")
    await sm.transition("start_planning")
    await sm.transition("plan_emitted")
    await sm.transition("dispatch_review")
    sm.ctx.data["plan_verdict"] = "APPROVED"
    await sm.transition("both_critics_done")
    await sm.transition("start_dispatch")
    await sm.transition("all_assigned")
    await sm.transition("first_result")
    await sm.transition("all_results_in")
    await sm.transition("verdict_repair")
    await sm.transition("final_review_pass")
    assert sm.state == State.DONE
    assert sm.is_terminal


@pytest.mark.asyncio
async def test_plan_revision_budget_guard() -> None:
    """`max_revisions` is rejected when the budget is not yet exhausted."""
    sm, _ = _make_sm()
    # Reach PLAN_REVISING with counter=0
    await sm.transition("start_analysis")
    await sm.transition("analysis_done")
    await sm.transition("start_planning")
    await sm.transition("plan_emitted")
    await sm.transition("dispatch_review")
    sm.ctx.data["plan_verdict"] = "REVISING"
    await sm.transition("both_critics_done")

    # Budget not exhausted → max_revisions rejected
    with pytest.raises(InvalidTransitionError):
        await sm.transition("max_revisions")
    assert sm.state == State.PLAN_REVISING

    # Bump the counter to 3 → max_revisions allowed
    sm.ctx.plan_revision_count = 3
    await sm.transition("max_revisions")
    assert sm.state == State.FAILED
