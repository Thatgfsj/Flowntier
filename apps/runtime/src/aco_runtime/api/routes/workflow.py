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
    options = OrchestratorOptions(
        speed=speed,
        enable_review=bool(req.get("enable_review", True)),
    )
    wf_id = f"wf_{uuid.uuid4().hex[:10]}"
    factory = get_factory()
    orchestrator = await factory(options)

    async def _run() -> None:
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


@router.post("/{wf_id}/cancel", status_code=204)
async def cancel_workflow(wf_id: str) -> None:
    task = _tasks.get(wf_id)
    if task is None or task.done():
        return
    task.cancel()
    logger.info("orchestrator {} cancelled", wf_id)
