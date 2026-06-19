"""Tests for FinalReviewerAgent (Phase 2 / C2).

The deterministic stub is what production uses in the absence
of a wired model, so we test it directly. Then a state-machine
test exercises the new FINAL_REVIEW transitions.
"""
from __future__ import annotations

import asyncio

import pytest

from aco_runtime_lib.agents import AgentRole, FinalReviewerAgent
from aco_runtime_lib.workflow.state_machine import State


def _run(reviewer: FinalReviewerAgent, ctx: dict) -> dict:
    return asyncio.run(reviewer.run(ctx)).data


# ── Heuristic reviewer ──────────────────────────────────────────


def test_empty_user_request_rejected() -> None:
    r = _run(FinalReviewerAgent(), {"user_request": "", "summary": "x", "task_results": []})
    assert r["verdict"] == "REJECT"
    assert r["confidence"] == 1.0
    assert any(i["severity"] == "MAJOR" for i in r["issues"])


def test_empty_summary_triggers_repair() -> None:
    r = _run(FinalReviewerAgent(), {"user_request": "do X", "summary": "", "task_results": [{"task_id": "T1", "status": "DONE"}]})
    assert r["verdict"] == "REPAIR"
    assert any("summary" in i["message"] for i in r["issues"])


def test_empty_task_results_triggers_repair() -> None:
    r = _run(FinalReviewerAgent(), {"user_request": "do X", "summary": "ok", "task_results": []})
    assert r["verdict"] == "REPAIR"
    assert any("task" in i["message"].lower() for i in r["issues"])


def test_all_failed_triggers_reject() -> None:
    tasks = [
        {"task_id": "T1", "status": "FAILED", "summary": "boom"},
        {"task_id": "T2", "status": "FAILED", "summary": "boom"},
    ]
    r = _run(FinalReviewerAgent(), {"user_request": "x", "summary": "y", "task_results": tasks})
    assert r["verdict"] == "REJECT"
    assert "all" in r["issues"][0]["message"].lower()


def test_partial_failure_triggers_repair() -> None:
    tasks = [
        {"task_id": "T1", "status": "DONE", "summary": "ok"},
        {"task_id": "T2", "status": "FAILED", "summary": "broken"},
    ]
    r = _run(FinalReviewerAgent(), {"user_request": "x", "summary": "y", "task_results": tasks})
    assert r["verdict"] == "REPAIR"
    issue_msgs = [i["message"] for i in r["issues"]]
    assert any("T2" in m for m in issue_msgs)


def test_happy_path_passes() -> None:
    tasks = [
        {"task_id": "T1", "status": "DONE", "summary": "ok"},
        {"task_id": "T2", "status": "DONE", "summary": "ok"},
    ]
    r = _run(FinalReviewerAgent(), {"user_request": "build X", "summary": "all done", "task_results": tasks})
    assert r["verdict"] == "PASS"
    assert r["confidence"] >= 0.5


def test_role_is_final_reviewer() -> None:
    assert FinalReviewerAgent().role == AgentRole.FINAL_REVIEWER


# ── State machine transitions for FINAL_REVIEW ─────────────────


@pytest.mark.asyncio
async def test_final_review_pass_path_reaches_done() -> None:
    """FINAL_REVIEW -> final_review_pass -> DONE."""
    from aco_runtime_lib.workflow.state_machine import (
        StateMachine, WorkflowCtx,
    )
    from aco_runtime_lib.event_bus import EventBus

    sm = StateMachine(
        ctx=WorkflowCtx(wf_id="wf_fr", actor="test"),
        bus=EventBus(),
        initial=State.FINAL_REVIEW,
    )
    await sm.transition("final_review_pass")
    assert sm.state == State.DONE
    assert sm.is_terminal


@pytest.mark.asyncio
async def test_final_review_reject_path_reaches_failed() -> None:
    from aco_runtime_lib.workflow.state_machine import (
        StateMachine, WorkflowCtx,
    )
    from aco_runtime_lib.event_bus import EventBus

    sm = StateMachine(
        ctx=WorkflowCtx(wf_id="wf_fr", actor="test"),
        bus=EventBus(),
        initial=State.FINAL_REVIEW,
    )
    await sm.transition("final_review_reject")
    assert sm.state == State.FAILED


@pytest.mark.asyncio
async def test_final_review_repair_path_with_budget() -> None:
    """REPAIR transition is gated by _under_repair_budget. Pass it
    and we land in REPAIRING."""
    from aco_runtime_lib.workflow.state_machine import (
        StateMachine, WorkflowCtx,
    )
    from aco_runtime_lib.event_bus import EventBus

    sm = StateMachine(
        ctx=WorkflowCtx(wf_id="wf_fr", actor="test"),
        bus=EventBus(),
        initial=State.FINAL_REVIEW,
    )
    # Repair budget default is 0 in StateMachine default; bump so
    # the guard passes.
    sm.ctx.repair_count = -1  # any negative triggers `_under_repair_budget`
    await sm.transition("final_review_repair")
    assert sm.state == State.REPAIRING