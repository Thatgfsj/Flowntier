"""End-to-end workflow test using the MockProvider.

Drives a full workflow from `REQ_RECEIVED` to `DONE` with no real
LLM calls. Validates:

* State machine transitions (re-uses `test_state_machine.py`).
* JSONL persistence writes every transition.
* Replay rebuilds the state machine from disk.
* Crash recovery reports only non-terminal workflows.
* All five agent roles (Chief, Planner, Worker, Critic, Reporter)
  produce well-formed output.

The MockProvider is **scripted**: each call's response is pre-registered
based on the input. This keeps the test deterministic and offline.
"""

from __future__ import annotations

import json
from pathlib import Path

import pytest
from aco_runtime_lib import EventBus, State, StateMachine, WorkflowCtx
from aco_runtime_lib.agents import (
    AgentRole,
    ChiefAgent,
    CriticAgent,
    PlannerAgent,
    ReporterAgent,
    WorkerAgent,
)
from aco_runtime_lib.providers import (
    MockProvider,
    default_router,
)
from aco_runtime_lib.workflow import (
    LogEntry,
    WorkflowLog,
    find_resumable,
    iter_entries_sync,
    last_entry,
)

# ── Mock scripting ───────────────────────────────────────────────

PLAN_MD = """\
# Plan: Add /login endpoint

## Goal
Implement POST /auth/login that returns a JWT for valid credentials.

## Architecture
- Use FastAPI for the route
- Use bcrypt for password hashing
- Use PyJWT for token signing

## Task Graph

| ID  | Title                  | Owner Role | Depends On | Est. Tokens |
|-----|------------------------|------------|------------|-------------|
| T1  | Implement login route  | backend    | —          | 8000        |

## APIs / Interfaces
- POST /auth/login → 200 {token: <jwt>} | 401 {error: "invalid"}

## Data Model
- users: id (uuid), email (text, unique), password_hash (text)

## Acceptance Criteria
1. POST /auth/login with valid creds returns 200 + JWT
2. POST /auth/login with invalid creds returns 401
3. JWT is HS256, 24h expiry

## Risks
- Rate-limiting not included in v0.1 (Phase 2)

## Out of Scope
- OAuth providers
"""


TASK_RESULT_JSON = json.dumps(
    {
        "task_id": "t1",
        "status": "DONE",
        "summary": "Implemented POST /auth/login with bcrypt + PyJWT. 12 tests pass.",
        "files_modified": [
            {"path": "src/auth/login.py", "lines_added": 87, "lines_removed": 0},
            {"path": "src/auth/login.test.py", "lines_added": 64, "lines_removed": 0},
        ],
        "tests_run": {"passed": 12, "failed": 0, "skipped": 0},
    }
)


CRITIC_PASS_JSON = json.dumps(
    {
        "verdict": "PASS",
        "confidence": 0.92,
        "issues": [],
        "summary": "No issues. Solid implementation.",
    }
)


def _build_scripted_mock() -> MockProvider:
    """Build a mock with responses for every LLM call in the happy path."""
    m = MockProvider()
    # 1. Planner generates a plan (first call)
    m.when(r"Planner for Agent Company OS", PLAN_MD, output_tokens=600)
    # 2. Chief may re-call for the plan; mock has the same matcher but
    # later entries win. Order matters: we add a chief matcher second.
    m.when(r"Chief Agent of ACO", PLAN_MD, output_tokens=600)
    # 3. Worker returns a TASK_RESULT for the login task
    m.when(r"# Task: Implement login route", TASK_RESULT_JSON, output_tokens=400)
    # 4. Critics (both A and B) review deliverables and return PASS
    m.when(r"^/im|^/im", CRITIC_PASS_JSON, output_tokens=200)
    m.when(r"## Subject", CRITIC_PASS_JSON, output_tokens=200)
    return m


def _make_chief(router, bus) -> ChiefAgent:
    return ChiefAgent(
        router=router,
        bus=bus,
        system_prompt="You are the Chief Agent of ACO.",
    )


def _make_critic(role: AgentRole, router, bus) -> CriticAgent:
    return CriticAgent(
        role=role,
        router=router,
        bus=bus,
        router_role="critic_a" if role == AgentRole.CRITIC_A else "critic_b",
        system_prompt=f"You are {role.value}, a focused reviewer.",
    )


# ── Tests ─────────────────────────────────────────────────────────


@pytest.mark.asyncio
async def test_e2e_happy_path_writes_full_jsonl(tmp_path: Path) -> None:
    """Drive a complete workflow; verify the JSONL log."""
    workflows_dir = tmp_path / "workflows"
    workflows_dir.mkdir()
    bus = EventBus()
    ctx = WorkflowCtx(wf_id="wf_e2e", actor="agent:chief")
    sm = StateMachine(ctx, bus, initial=State.REQ_RECEIVED)
    log = WorkflowLog(workflows_dir / f"{sm.ctx.wf_id}.jsonl")
    await log.open()

    # Wire a tracker so every transition lands in the log
    original = sm.transition

    async def tracked(event: str) -> State:
        new_state = await original(event)
        await log.append(
            wf_id=sm.ctx.wf_id,
            from_state=None,
            to_state=new_state,
            event=event,
            actor=sm.ctx.actor,
        )
        return new_state

    sm.transition = tracked  # type: ignore[method-assign]

    try:
        # Phase 1
        await sm.transition("start_analysis")
        await sm.transition("analysis_done")
        # Phase 2
        await sm.transition("start_planning")
        await sm.transition("plan_emitted")
        # Phase 3
        await sm.transition("dispatch_review")
        sm.ctx.data["critic_a_verdict"] = "PASS"
        sm.ctx.data["critic_b_verdict"] = "PASS"
        sm.ctx.data["plan_verdict"] = "APPROVED"
        await sm.transition("both_critics_done")
        # Phase 4
        await sm.transition("start_dispatch")
        await sm.transition("all_assigned")
        # Phase 5
        await sm.transition("first_result")
        await sm.transition("all_results_in")
        # Phase 6
        await sm.transition("verdict_pass")
        # Phase 8
        await sm.transition("report_emitted")
        # Phase 8b — Final Review (PASS path).
        await sm.transition("final_review_pass")
        assert sm.state == State.DONE
    finally:
        sm.transition = original  # type: ignore[method-assign]
        await log.close()

    # Verify the JSONL has every transition (now 13 — added
    # final_review_pass transition in Phase 2 / C2).
    entries = list(iter_entries_sync(workflows_dir / "wf_e2e.jsonl"))
    assert len(entries) == 13
    assert entries[-1].to_state == "DONE"

    # And recovery should NOT report this workflow (it is terminal)
    resumable = find_resumable(workflows_dir)
    assert resumable == []


@pytest.mark.asyncio
async def test_e2e_persistence_then_replay(tmp_path: Path) -> None:
    """Write a partial log, then read it back and verify reconstruction."""
    workflows_dir = tmp_path / "workflows"
    workflows_dir.mkdir()

    # Write a partial workflow by hand (3 transitions, leaving it in PLAN_DRAFTED)
    log_path = workflows_dir / "wf_partial.jsonl"
    log = WorkflowLog(log_path)
    await log.open()
    await log.append("wf_partial", None, State.REQ_RECEIVED, "user_input", "agent:user")
    await log.append(
        "wf_partial", State.REQ_RECEIVED, State.REQ_ANALYZING, "start_analysis", "agent:chief"
    )
    await log.append(
        "wf_partial", State.REQ_ANALYZING, State.REQ_CLARIFIED, "analysis_done", "agent:chief"
    )
    await log.append(
        "wf_partial", State.REQ_CLARIFIED, State.PLAN_DRAFTING, "start_planning", "agent:chief"
    )
    await log.append(
        "wf_partial", State.PLAN_DRAFTING, State.PLAN_DRAFTED, "plan_emitted", "agent:chief"
    )
    await log.close()

    # The last entry should be PLAN_DRAFTED
    last = last_entry(log_path)
    assert last is not None
    assert last.to_state == "PLAN_DRAFTED"

    # Recovery should report this workflow (non-terminal)
    resumable = find_resumable(workflows_dir)
    assert len(resumable) == 1
    assert resumable[0].wf_id == "wf_partial"
    assert resumable[0].last_state == State.PLAN_DRAFTED
    assert resumable[0].entry_count == 5


@pytest.mark.asyncio
async def test_e2e_with_mock_agents_full_loop(tmp_path: Path) -> None:
    """Run a full workflow with MockProvider driving the agents."""
    mock = _build_scripted_mock()
    router = default_router()
    # Inject the scripted mock into the router
    router.register("mock", mock)

    bus = EventBus()
    ctx = WorkflowCtx(wf_id="wf_mock", actor="agent:chief")
    sm = StateMachine(ctx, bus, initial=State.REQ_RECEIVED)

    # The Chief agent (powered by mock)
    chief = _make_chief(router, bus)
    critic_a = _make_critic(AgentRole.CRITIC_A, router, bus)
    critic_b = _make_critic(AgentRole.CRITIC_B, router, bus)
    worker = WorkerAgent(router=router)
    planner = PlannerAgent(router=router)
    reporter = ReporterAgent()

    # 1. Chief reads the user request
    chief_result = await chief.run({"user_request": "Add a /login endpoint"})
    assert "kind" in chief_result.data

    # 2. Planner generates a plan
    plan_result = await planner.run({"user_request": "Add a /login endpoint"})
    assert "plan_md" in plan_result.data
    assert "Task Graph" in plan_result.data["plan_md"]
    tasks = plan_result.data["tasks"]
    assert len(tasks) == 1
    assert tasks[0]["id"] == "T1"
    sm.ctx.data["plan_md"] = plan_result.data["plan_md"]
    sm.ctx.data["tasks"] = tasks

    # 3. Walk the state machine through PLAN_APPROVED
    await sm.transition("start_analysis")
    await sm.transition("analysis_done")
    await sm.transition("start_planning")
    await sm.transition("plan_emitted")
    await sm.transition("dispatch_review")

    # Critics review the plan
    crit_a_result = await critic_a.run({"subject": "Plan", "ask": "Is this sound?", "files": []})
    crit_b_result = await critic_b.run({"subject": "Plan", "ask": "Is this sound?", "files": []})
    assert crit_a_result.data["verdict"] == "PASS"
    assert crit_b_result.data["verdict"] == "PASS"
    sm.ctx.data["critic_a_verdict"] = "PASS"
    sm.ctx.data["critic_b_verdict"] = "PASS"
    sm.ctx.data["plan_verdict"] = "APPROVED"
    await sm.transition("both_critics_done")
    assert sm.state == State.PLAN_APPROVED

    # 4. Dispatch + develop
    await sm.transition("start_dispatch")
    await sm.transition("all_assigned")
    await sm.transition("first_result")

    # Worker runs the task
    worker_result = await worker.run(
        {
            "task_id": "t1",
            "title": "Implement login route",
            "objective": "Implement POST /auth/login with bcrypt + PyJWT",
            "interfaces": {"consumes": ["users table"], "produces": ["/auth/login"]},
            "dependencies": [],
            "constraints": ["bcrypt cost 12"],
            "deliverables": ["src/auth/login.py", "src/auth/login.test.py"],
            "context_budget_tokens": 8000,
        }
    )
    assert worker_result.data["status"] == "DONE"
    assert len(worker_result.data["files_modified"]) == 2
    sm.ctx.data["task_results"] = [worker_result.data]

    await sm.transition("all_results_in")

    # Critics review the deliverable
    crit_a_result = await critic_a.run(
        {"subject": "Login impl", "ask": "Any bugs?", "files": ["src/auth/login.py"]}
    )
    crit_b_result = await critic_b.run(
        {"subject": "Login impl", "ask": "Any bugs?", "files": ["src/auth/login.py"]}
    )
    assert crit_a_result.data["verdict"] == "PASS"
    assert crit_b_result.data["verdict"] == "PASS"
    sm.ctx.data["critic_a_verdict"] = "PASS"
    sm.ctx.data["critic_b_verdict"] = "PASS"
    sm.ctx.data["any_major_issue"] = False
    await sm.transition("verdict_pass")

    # 5. Delivery
    await sm.transition("report_emitted")
    # 6. Final review (Phase 2 / C2).
    await sm.transition("final_review_pass")
    assert sm.state == State.DONE

    # Reporter composes the summary
    report = await reporter.run(
        {
            "log": [],
            "task_results": sm.ctx.data["task_results"],
            "workflow_status": "DONE",
        }
    )
    summary = report.data["summary"]
    assert "Delivery Summary" in summary
    assert "src/auth/login.py" in summary

    # The mock was called: planner, chief, critic_a (x2), critic_b (x2), worker
    assert len(mock.calls) >= 5


@pytest.mark.asyncio
async def test_e2e_replay_rebuilds_state_machine(tmp_path: Path) -> None:
    """Write a log; read it back; reconstruct the state machine."""
    workflows_dir = tmp_path / "workflows"
    workflows_dir.mkdir()

    log = WorkflowLog(workflows_dir / "wf_replay.jsonl")
    await log.open()
    # Walk a workflow halfway and stop.
    await log.append("wf_replay", None, State.REQ_RECEIVED, "user_input", "agent:user")
    await log.append(
        "wf_replay", State.REQ_RECEIVED, State.REQ_ANALYZING, "start_analysis", "agent:chief"
    )
    await log.append(
        "wf_replay", State.REQ_ANALYZING, State.REQ_CLARIFIED, "analysis_done", "agent:chief"
    )
    await log.append(
        "wf_replay", State.REQ_CLARIFIED, State.PLAN_DRAFTING, "start_planning", "agent:chief"
    )
    await log.append(
        "wf_replay", State.PLAN_DRAFTING, State.PLAN_DRAFTED, "plan_emitted", "agent:chief"
    )
    await log.close()

    # Replay: read the entries and reconstruct the path
    entries: list[LogEntry] = list(iter_entries_sync(workflows_dir / "wf_replay.jsonl"))
    path = [e.to_state for e in entries]
    assert path == [
        "REQ_RECEIVED",
        "REQ_ANALYZING",
        "REQ_CLARIFIED",
        "PLAN_DRAFTING",
        "PLAN_DRAFTED",
    ]

    # We can drive a fresh state machine to the last logged state.
    bus = EventBus()
    ctx = WorkflowCtx(wf_id="wf_replay", actor="agent:chief")
    sm = StateMachine(ctx, bus, initial=State.REQ_RECEIVED)

    # In production, the recovery flow would re-derive the event
    # from (from_state, to_state); for the test we use the known
    # event names from the writes above.
    await sm.transition("start_analysis")
    await sm.transition("analysis_done")
    await sm.transition("start_planning")
    await sm.transition("plan_emitted")
    assert sm.state == State.PLAN_DRAFTED


@pytest.mark.asyncio
async def test_e2e_recovery_finds_non_terminal(tmp_path: Path) -> None:
    """Two workflows on disk: one DONE, one stuck. Recovery reports the stuck one."""
    workflows_dir = tmp_path / "workflows"
    workflows_dir.mkdir()

    # Workflow 1: complete
    log1 = WorkflowLog(workflows_dir / "wf_done.jsonl")
    await log1.open()
    for s in (
        State.REQ_RECEIVED,
        State.REQ_ANALYZING,
        State.REQ_CLARIFIED,
        State.PLAN_DRAFTING,
        State.PLAN_DRAFTED,
        State.PLAN_UNDER_REVIEW,
        State.PLAN_APPROVED,
        State.DISPATCHING,
        State.AWAITING_WORKERS,
        State.DEVELOPING,
        State.REVIEWING,
        State.DELIVERING,
        State.DONE,
    ):
        await log1.append("wf_done", None, s, "walk", "agent:chief")
    await log1.close()

    # Workflow 2: stuck in REVIEWING
    log2 = WorkflowLog(workflows_dir / "wf_stuck.jsonl")
    await log2.open()
    for s in (
        State.REQ_RECEIVED,
        State.REQ_ANALYZING,
        State.REQ_CLARIFIED,
        State.PLAN_DRAFTING,
        State.PLAN_DRAFTED,
        State.PLAN_UNDER_REVIEW,
        State.PLAN_APPROVED,
        State.DISPATCHING,
        State.AWAITING_WORKERS,
        State.DEVELOPING,
        State.REVIEWING,
    ):
        await log2.append("wf_stuck", None, s, "walk", "agent:chief")
    await log2.close()

    resumable = find_resumable(workflows_dir)
    assert len(resumable) == 1
    assert resumable[0].wf_id == "wf_stuck"
    assert resumable[0].last_state == State.REVIEWING
    assert resumable[0].entry_count == 11
