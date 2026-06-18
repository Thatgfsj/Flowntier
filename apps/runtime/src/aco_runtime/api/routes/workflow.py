"""Workflow HTTP routes. Real implementations for v0.2.

`POST /api/workflow`         — start a new workflow; runs the
                                orchestrator in the background; emits
                                events to the shared bus (consumed by
                                the WebSocket).
`GET  /api/workflow/{id}`     — fetch the current orchestrator
                                state for a workflow run (best-effort
                                in-memory; persisted in v0.3).
"""

from __future__ import annotations

import asyncio
import time
import uuid
from collections.abc import Awaitable, Callable
from typing import Any

from fastapi import APIRouter, HTTPException
from loguru import logger

from aco_runtime_lib.workflow import (
    OrchestratorOptions,
    OrchestratorResult,
    WorkflowOrchestrator,
)

router = APIRouter()

# ── In-memory state ──────────────────────────────────────────────

OrchestratorFactory = Callable[
    [OrchestratorOptions | None], Awaitable[WorkflowOrchestrator]
]
"""Factory that takes options and returns an initialized orchestrator.

Async because the orchestrator is a Python object that may need
async init in future versions.
"""

_factory: OrchestratorFactory | None = None
_runs: dict[str, OrchestratorResult] = {}
_tasks: dict[str, asyncio.Task[None]] = {}
_plan_data: dict[str, dict[str, Any]] = {}
"""Per-workflow plan data captured at the planning phase.

Keys: wf_id.
Values: {plan_md, parsed_plan, parse_error, validation, validation_error, tasks}.
The UI's /plan endpoint reads this to render the task list before the
workflow finishes."""

# Serializes workflow runs. Multiple concurrent workflows against the
# same ModelRouter (single shared API key + single TCP pool) cause
# rate-limiting and unpredictable latency. A Lock keeps one workflow
# in-flight at a time; additional POSTs are accepted immediately but
# block at `_run` until the previous workflow finishes. The POST itself
# still returns 202 within milliseconds — only the actual orchestrator
# execution is serialized.
_run_lock: asyncio.Lock = asyncio.Lock()


def bind_orchestrator_factory(factory: OrchestratorFactory) -> None:
    global _factory
    _factory = factory


def get_factory() -> OrchestratorFactory:
    if _factory is None:
        raise HTTPException(status_code=503, detail="orchestrator not ready")
    return _factory


# ── Endpoints ────────────────────────────────────────────────────


class NewWorkflowRequest:
    text: str
    project_id: str | None = None
    speed: str = "balanced"
    enable_review: bool = True


@router.post("", status_code=202)
async def start_workflow(req: dict[str, Any]) -> dict[str, Any]:
    """Kick off a new workflow run. Returns immediately with a wf_id;
    the orchestrator runs in the background and pushes events to the
    bus, which the WebSocket endpoint streams to clients.
    """
    text = (req.get("text") or "").strip()
    if not text:
        raise HTTPException(status_code=400, detail="text is required")
    speed = req.get("speed", "balanced")
    if speed not in ("fast", "balanced", "thorough"):
        speed = "balanced"

    async def _on_event(event: Any) -> None:
        """Capture plan data as soon as the planning phase completes."""
        if event.kind == "transition" and event.to_state == "PLAN_DRAFTED":
            ctx = orchestrator._ctx  # type: ignore[attr-defined]
            _plan_data[wf_id] = {
                "plan_md": ctx.data.get("plan_md"),
                "parsed_plan": ctx.data.get("parsed_plan"),
                "parse_error": ctx.data.get("parse_error"),
                "validation": ctx.data.get("validation"),
                "validation_error": ctx.data.get("validation_error"),
                "tasks": ctx.data.get("tasks") or [],
            }
            logger.info("captured plan data for {}", wf_id)
        elif event.kind == "task_status":
            # Track live task status so /plan can report current state
            # even before the workflow finishes. Note: the dataclass
            # field is `task_state` (renamed to avoid collision with
            # the `task_status()` factory). The wire format keeps
            # `task_status` (rewritten by `_serialize`).
            entry = _plan_data.setdefault(wf_id, {})
            statuses = entry.setdefault("task_statuses", {})
            statuses[event.task_id] = {
                "status": event.task_state,
                "title": event.task_title,
                "summary": event.task_summary,
                "files": list(event.task_files or []),
            }

    options = OrchestratorOptions(
        speed=speed,
        enable_review=bool(req.get("enable_review", True)),
        on_event=_on_event,
    )
    wf_id = f"wf_{uuid.uuid4().hex[:10]}"
    factory = get_factory()
    orchestrator = await factory(options)

    async def _run() -> None:
        # Serialize: wait for any in-flight workflow to finish first.
        # See `_run_lock` docstring for rationale.
        async with _run_lock:
            try:
                logger.info("orchestrator {} start", wf_id)
                result = await orchestrator.run(wf_id, text)
                _runs[wf_id] = result
                logger.info("orchestrator {} done: {}", wf_id, result.final_state.value)
            except Exception as e:  # noqa: BLE001
                logger.exception("orchestrator {} failed: {}", wf_id, e)
                _runs[wf_id] = OrchestratorResult(
                    wf_id=wf_id,
                    final_state=type("S", (), {"value": "FAILED"})(),  # noqa
                    summary=f"orchestrator error: {e}",
                )

    task = asyncio.create_task(_run(), name=f"orch-{wf_id}")
    _tasks[wf_id] = task
    return {
        "id": wf_id,
        "status": "started",
        "submitted_at": int(time.time()),
    }


@router.get("/{wf_id}")
async def get_workflow(wf_id: str) -> dict[str, Any]:
    if wf_id not in _runs:
        return {
            "id": wf_id,
            "state": "PENDING" if wf_id in _tasks else "UNKNOWN",
            "phase": "running" if wf_id in _tasks else "unknown",
        }
    result = _runs[wf_id]
    return {
        "id": result.wf_id,
        "state": result.final_state.value,
        "summary": result.summary,
        "task_results": result.task_results,
    }


@router.get("/{wf_id}/summary")
async def get_workflow_summary(wf_id: str) -> dict[str, Any]:
    """Return the final summary if the workflow has finished."""
    if wf_id not in _runs:
        raise HTTPException(status_code=404, detail="workflow not found")
    return {
        "id": wf_id,
        "summary": _runs[wf_id].summary,
    }


@router.get("/{wf_id}/plan")
async def get_workflow_plan(wf_id: str) -> dict[str, Any]:
    """Return the parsed plan for the workflow.

    Used by the desktop UI to render the task list (right panel)
    and the Plan tab (Phase 2.4 graph). Available as soon as the
    orchestrator's planning phase completes; for in-flight
    workflows, returns whatever's been captured so far.
    """
    data = _plan_data.get(wf_id)
    if data is None:
        # The workflow might still be in REQ_RECEIVED / REQ_ANALYZING.
        # Return an empty plan so the UI can render a "loading" state.
        return {
            "id": wf_id,
            "parsed_plan": None,
            "tasks": [],
            "task_statuses": {},
            "parse_error": None,
            "validation_error": None,
            "status": "pending",
        }
    return {
        "id": wf_id,
        "parsed_plan": data.get("parsed_plan"),
        "tasks": data.get("tasks") or [],
        "task_statuses": data.get("task_statuses", {}),
        "parse_error": data.get("parse_error"),
        "validation_error": data.get("validation_error"),
        "status": "ready",
    }


@router.post("/{wf_id}/cancel", status_code=204)
async def cancel_workflow(wf_id: str) -> None:
    task = _tasks.get(wf_id)
    if task is None or task.done():
        return
    task.cancel()
    logger.info("orchestrator {} cancelled", wf_id)
