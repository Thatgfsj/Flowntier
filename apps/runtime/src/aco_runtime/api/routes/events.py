"""WebSocket endpoint for streaming workflow events to the Tauri webview.

Each connected client subscribes to a per-client queue on the
shared event bus. The bus is wired in by `bind_bus()` at app startup
(see `aco_runtime.main`).
"""

from __future__ import annotations

import asyncio
import json
from datetime import UTC, datetime
from typing import Any

from fastapi import APIRouter, WebSocket, WebSocketDisconnect

from aco_runtime_lib import EventBus, WfEvent

router = APIRouter()

_bus: EventBus | None = None


def bind_bus(bus: EventBus) -> None:
    global _bus
    _bus = bus


def _serialize(event: WfEvent) -> dict[str, Any]:
    """Drop None-valued fields so the JSON is small."""
    out: dict[str, Any] = {"kind": event.kind, "ts": event.ts}
    for f in event.__dataclass_fields__:
        if f in ("kind", "ts"):
            continue
        v = getattr(event, f)
        if v is not None:
            out[f] = v
    return out


@router.websocket("/stream")
async def stream_events(websocket: WebSocket) -> None:
    """Stream every WfEvent published on the shared bus.

    The runtime accepts the connection, sends a hello, then
    subscribes to the bus and forwards every event as a JSON
    message until the client disconnects.
    """
    await websocket.accept()
    bus = _bus
    if bus is None:
        await websocket.close(code=1011, reason="event bus not initialized")
        return
    queue = await bus.subscribe()
    try:
        await websocket.send_json(
            {
                "kind": "console",
                "agent_id": "agent:system",
                "level": "info",
                "message": "aco-runtime connected",
                "ts": datetime.now(UTC).isoformat(),
            }
        )
        # Forward events from the bus to the client until disconnect.
        # We use a small sleep to yield control and let the event
        # loop dispatch new items from the queue.
        while True:
            try:
                event = await asyncio.wait_for(queue.get(), timeout=1.0)
            except asyncio.TimeoutError:
                # Send a heartbeat to keep the connection alive.
                await websocket.send_json(
                    {
                        "kind": "heartbeat",
                        "ts": datetime.now(UTC).isoformat(),
                    }
                )
                continue
            await websocket.send_json(_serialize(event))
    except WebSocketDisconnect:
        return
    finally:
        await bus.unsubscribe(queue)
