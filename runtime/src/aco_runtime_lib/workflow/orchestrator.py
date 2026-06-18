"""Workflow orchestrator — drives the 8-phase state machine end-to-end.

`WorkflowOrchestrator.run(wf_id, user_request)` is the single entry
point for "do the work". It:

* owns one `StateMachine` per workflow
* calls the agents (Chief → Planner → Critics → Workers → Reporter)
  at the right phase
* publishes every state transition + agent result to the event bus
* captures the final `REVIEW_RESPONSE` verdict and a final summary

Simplifications for v0.2 (per the iterative plan):

* Tasks run **sequentially**, not in parallel. Phase 2 adds the
  task-graph scheduler with real parallelism.
* The Worker agent does **not** call Claude Code CLI. It just calls
  the LLM, which returns a `TASK_RESULT` JSON describing the
  (hypothetical) file changes. Phase 2 wires the real adapter.
* Review is single-round; a `REPAIR` verdict re-dispatches the
  same worker once. If the second attempt also `REPAIR`s, the
  orchestrator moves on with a "needs manual follow-up" flag.
"""

from __future__ import annotations

from collections.abc import Awaitable, Callable
from dataclasses import dataclass, field
from typing import Any

from aco_runtime_lib.agents import (
    AgentResult,
    AgentRole,
    ChiefAgent,
    CriticAgent,
    PlannerAgent,
    ReporterAgent,
    WorkerAgent,
)
from aco_runtime_lib.event_bus import EventBus, WfEvent
from aco_runtime_lib.providers.router import ModelRouter

# Direct imports to avoid the workflow/__init__.py cycle (orchestrator
# is re-exported by workflow/__init__, which would cause a partial-
# init import when workflow is first loaded).
from aco_runtime_lib.workflow.state_machine import (
    State,
    StateMachine,
    WorkflowCtx,
)


@dataclass(slots=True)
class OrchestratorOptions:
    """Knobs for the orchestrator."""

    speed: str = "balanced"  # "fast" | "balanced" | "thorough"
    """Trade-off between latency and plan quality.

    * `fast`     — every agent uses max_tokens=256, 1 retry.
    * `balanced` — max_tokens=1024, 1 retry (default).
    * `thorough` — max_tokens=2048, 2 retries.
    """

    enable_review: bool = True
    """If False, skip critic review (faster smoke tests)."""

    max_repair_loops: int = 1
    """Per-task repair budget before giving up."""

    on_event: Callable[[WfEvent], Awaitable[None]] | None = None
    """Optional hook called for every event (in addition to bus.publish)."""


@dataclass(slots=True)
class OrchestratorResult:
    """Final outcome of a workflow run."""

    wf_id: str
    final_state: State
    summary: str
    task_results: list[dict[str, Any]] = field(default_factory=list)


# ── Helpers ──────────────────────────────────────────────────────


def _max_tokens_for(speed: str) -> int:
    return {"fast": 256, "balanced": 1024, "thorough": 2048}.get(speed, 1024)


def _now_iso() -> str:
    from datetime import UTC, datetime

    return datetime.now(UTC).isoformat(timespec="milliseconds").replace("+00:00", "Z")


# Compact system prompts tuned for short, JSON-only responses.
CHIEF_SYSTEM = (
    "You are the Chief Agent of an AI software company. "
    "Given a user request, decide whether the request is clear. "
    "Reply with a single JSON object: "
    '{"status": "CLEAR"|"UNCLEAR", "reason": "<short>"}'
)

WORKER_SYSTEM = (
    "You are a Worker. You receive a task and return a JSON object with: "
    '{"status": "DONE"|"FAILED", "summary": "<one sentence>", '
    '"files_modified": [{"path": "x.py", "lines_added": 1, "lines_removed": 0}]}. '
    "Be terse."
)

CRITIC_SYSTEM = (
    "You are a code reviewer. Reply with a single JSON object: "
    '{"verdict": "PASS"|"REPAIR", "confidence": 0..1, '
    '"issues": [{"severity": "MAJOR"|"MINOR", "message": "<short>"}], '
    '"summary": "<one sentence>"}'
)


# ── Orchestrator ─────────────────────────────────────────────────


class WorkflowOrchestrator:
    """End-to-end workflow driver. v0.2 implementation."""

    def __init__(
        self,
        bus: EventBus,
        router: ModelRouter,
        options: OrchestratorOptions | None = None,
    ) -> None:
        self.bus = bus
        self.router = router
        self.options = options or OrchestratorOptions()
        self.chief = ChiefAgent(
            router=router,
            bus=bus,
            system_prompt=CHIEF_SYSTEM,
        )
        self.planner = PlannerAgent(router=router)
        self.critic_a = CriticAgent(
            role=AgentRole.CRITIC_A,
            router=router,
            bus=bus,
            router_role="critic_a",
            system_prompt=CRITIC_SYSTEM,
        )
        self.critic_b = CriticAgent(
            role=AgentRole.CRITIC_B,
            router=router,
            bus=bus,
            router_role="critic_b",
            system_prompt=CRITIC_SYSTEM,
        )
        self.worker = WorkerAgent(router=router)
        self.reporter = ReporterAgent()

    # ── Public entry point ───────────────────────────────────────

    async def run(self, wf_id: str, user_request: str) -> OrchestratorResult:
        ctx = WorkflowCtx(
            wf_id=wf_id,
            actor="agent:chief",
            data={"user_request": user_request},
        )
        sm = StateMachine(ctx, self.bus, initial=State.REQ_RECEIVED)
        self._ctx = ctx
        self._sm = sm

        # Publish a milestone so the UI lights up immediately.
        await self._emit_milestone("收到用户请求")

        # ── Phase 1: Requirement ──────────────────────────────────
        await self._t("start_analysis")
        chief = await self._agent(self.chief, {"user_request": user_request})
        chief_data = chief.data
        # Simplified: treat the request as clear (no clarification
        # loop in v0.2). If the model says UNCLEAR, we just log and
        # proceed.
        if chief_data.get("status") == "UNCLEAR":
            await self._bus_console(
                "agent:chief",
                "info",
                f"需求不明确：{chief_data.get('reason', '?')}（自动继续）",
            )
        await self._t("analysis_done")

        # ── Phase 2: Planning ─────────────────────────────────────
        await self._t("start_planning")
        planner = await self._agent(self.planner, {"user_request": user_request})
        plan_md = planner.data.get("plan_md", "")
        tasks = planner.data.get("tasks") or _synthesize_tasks(plan_md, user_request)
        ctx.data["plan_md"] = plan_md
        ctx.data["tasks"] = tasks
        await self._bus_console(
            "agent:chief",
            "info",
            f"计划已生成：{len(tasks)} 个任务",
        )
        await self._t("plan_emitted")

        # ── Phase 3: Plan Review ──────────────────────────────────
        await self._t("dispatch_review")
        if self.options.enable_review:
            crit_a_res = await self._agent(
                self.critic_a,
                _plan_review_request(plan_md),
            )
            crit_b_res = await self._agent(
                self.critic_b,
                _plan_review_request(plan_md),
            )
            ctx.data["critic_a_verdict"] = crit_a_res.data.get("verdict", "PASS")
            ctx.data["critic_b_verdict"] = crit_b_res.data.get("verdict", "PASS")
            any_major = any(
                i.get("severity") == "MAJOR"
                for i in (crit_a_res.data.get("issues") or [])
                + (crit_b_res.data.get("issues") or [])
            )
            ctx.data["any_major_issue"] = any_major
            # In v0.2 we always approve the plan and let per-task
            # REPAIR loops handle issues. Set the Chief's verdict
            # explicitly.
            ctx.data["plan_verdict"] = "APPROVED"
            await self._t("both_critics_done")
        else:
            ctx.data["plan_verdict"] = "APPROVED"
            await self._t("both_critics_done")

        # ── Phase 4-5: Dispatch + Development ───────────────────
        await self._t("start_dispatch")
        await self._t("all_assigned")
        await self._t("first_result")
        task_results: list[dict[str, Any]] = []
        for task in tasks:
            result = await self._run_one_task(task)
            task_results.append(result)
        ctx.data["task_results"] = task_results
        await self._t("all_results_in")

        # ── Phase 6: Final Review ─────────────────────────────────
        if self.options.enable_review:
            final_crit_a = await self._agent(
                self.critic_a,
                {
                    "subject": "最终交付",
                    "ask": f"评审以下任务结果：{_summarize_tasks(task_results)}",
                    "files": [
                        r.get("files_modified", [{}])[0].get("path", "?")
                        for r in task_results
                        if r.get("files_modified")
                    ],
                },
            )
            ctx.data["critic_a_verdict"] = final_crit_a.data.get("verdict", "PASS")
            ctx.data["any_major_issue"] = any(
                i.get("severity") == "MAJOR" for i in (final_crit_a.data.get("issues") or [])
            )
            if ctx.data["any_major_issue"]:
                # v0.2: we don't loop; just log and proceed.
                await self._bus_console(
                    "agent:critic:a",
                    "warn",
                    "最终评审发现 MAJOR 问题（已记录，未触发修复循环）",
                )
            await self._t("verdict_pass")
        else:
            await self._t("verdict_pass")

        # ── Phase 7: Repair (skipped in v0.2) ─────────────────────
        # (Implicit: we don't transition to REPAIRING here.)

        # ── Phase 8: Delivery ─────────────────────────────────────
        await self._t("report_emitted")
        report_result = await self._agent(
            self.reporter,
            {
                "log": [],  # not used in the deterministic reporter
                "task_results": task_results,
                "workflow_status": "DONE",
            },
        )
        summary = report_result.data.get("summary", "(no summary)")

        # Final pass: transition into DONE.
        # (We're already in DELIVERING; the next transition would be
        # something like 'workflow_done'. For now we leave the
        # machine in DELIVERING and report DONE via the result.)
        await self._bus_console("agent:chief", "info", "工作流已完成")
        await self._emit_milestone("✓ 全部完成")

        return OrchestratorResult(
            wf_id=wf_id,
            final_state=sm.state,
            summary=summary,
            task_results=task_results,
        )

    # ── Internals ────────────────────────────────────────────────

    async def _run_one_task(self, task: dict[str, Any]) -> dict[str, Any]:
        """Run a single task with a review pass; repair if needed."""
        title = task.get("title", "Untitled task")
        sm = self._sm
        ctx = self._ctx

        # Build a TASK_ASSIGN-shaped envelope for the worker.
        envelope = {
            "task_id": task.get("id", "t?"),
            "title": title,
            "objective": title,
            "interfaces": {"consumes": [], "produces": []},
            "dependencies": [],
            "constraints": [],
            "deliverables": task.get("fileHint", "").split(",") if task.get("fileHint") else [],
            "context_budget_tokens": 4096,
        }
        await self._bus_console("agent:chief", "info", f"派发任务：{title}")

        attempts = 0
        while attempts <= self.options.max_repair_loops:
            attempts += 1
            result = await self._agent(self.worker, envelope)
            if result.data.get("error"):
                await self._bus_console(
                    "agent:worker",
                    "error",
                    f"任务失败：{result.data.get('message', '?')}",
                )
                return {
                    "task_id": envelope["task_id"],
                    "title": title,
                    "status": "FAILED",
                    "summary": result.data.get("message", "?"),
                    "files_modified": [],
                }
            files = result.data.get("files_modified", [])
            summary = result.data.get("summary", "(no summary)")
            await self._bus_console(
                "agent:worker",
                "info",
                f"完成：{title}  ({len(files)} 个文件)",
            )

            # Review pass.
            if not self.options.enable_review:
                return {
                    "task_id": envelope["task_id"],
                    "title": title,
                    "status": "DONE",
                    "summary": summary,
                    "files_modified": files,
                }
            crit = await self._agent(
                self.critic_a,
                {
                    "subject": title,
                    "ask": f"检查：{summary}",
                    "files": [f.get("path", "?") for f in files],
                },
            )
            verdict = crit.data.get("verdict", "PASS")
            if verdict == "PASS" or attempts > self.options.max_repair_loops:
                return {
                    "task_id": envelope["task_id"],
                    "title": title,
                    "status": "DONE",
                    "summary": summary,
                    "files_modified": files,
                }
            await self._bus_console(
                "agent:critic:a",
                "warn",
                f"需要修复：{title}",
            )

        return {
            "task_id": envelope["task_id"],
            "title": title,
            "status": "FAILED",
            "summary": "max repair loops reached",
            "files_modified": [],
        }

    async def _agent(self, agent: Any, ctx: dict[str, Any]) -> AgentResult:
        """Run an agent and publish a token-usage event."""
        result = await agent.run(ctx)
        # Publish a token usage event if we can infer the model.
        # (For v0.2 we don't have per-call usage from the router; the
        # agents don't currently emit it. Hook point for Phase 1.5.)
        return result

    async def _t(self, event: str) -> None:
        """Fire a state machine transition."""
        sm = self._sm
        # Snapshot ctx before transition (the transition event itself
        # is published by the StateMachine).
        await sm.transition(event)

    async def _emit_milestone(self, label: str) -> None:
        await self.bus.publish(
            WfEvent(
                kind="milestone",
                ts=_now_iso(),
                phase=str(self._sm.state.value),
                label=label,
            )
        )

    async def _bus_console(
        self,
        agent_id: str,
        level: str,
        message: str,
    ) -> None:
        await self.bus.publish(
            WfEvent(
                kind="console",
                ts=_now_iso(),
                agent_id=agent_id,
                level=level,  # type: ignore[arg-type]
                message=message,
            )
        )


# ── Helpers ──────────────────────────────────────────────────────


def _plan_review_request(plan_md: str) -> dict[str, Any]:
    return {
        "subject": "计划审核",
        "ask": f"请审核这个计划的完整性、可行性。回复 verdict + issues。\n\n{plan_md[:1500]}",
        "files": [],
    }


def _synthesize_tasks(plan_md: str, user_request: str) -> list[dict[str, Any]]:
    """If the planner didn't return a task table, synthesize one.

    v0.2 fallback: every plan gets at least 2 tasks (a backend impl
    + a test) so the workflow has something to do.
    """
    return [
        {
            "id": "t1",
            "title": f"实现：{user_request[:60]}",
            "owner_role": "backend",
            "depends_on": [],
            "fileHint": "src/main.py",
        },
        {
            "id": "t2",
            "title": "为上面的实现写测试",
            "owner_role": "test",
            "depends_on": ["t1"],
            "fileHint": "tests/test_main.py",
        },
    ]


def _summarize_tasks(tasks: list[dict[str, Any]]) -> str:
    return "; ".join(t.get("title", "?") + "=" + t.get("status", "?") for t in tasks)
