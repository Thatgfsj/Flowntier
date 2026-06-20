"""FastAPI entry point for the Python AI runtime sidecar.

This process is launched by the Tauri shell (`tauri-plugin-shell`)
and communicates with the desktop app via:
  - HTTP for RPC (workflow start, plugin calls)
  - WebSocket for streaming events (workflow transitions, console)
  - JSON-RPC 2.0 over stdio for plugin IPC (the Tauri sidecar manages
    this for builtin plugins).

The actual workflow engine lives in the `runtime/` workspace member
(`runtime/workflow/...`). This file is the **thin** HTTP shell.
"""

from __future__ import annotations

import asyncio
import os
import signal
import sys
from contextlib import asynccontextmanager
from typing import AsyncIterator

import uvicorn
from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
from loguru import logger

from aco_runtime_lib import EventBus
from aco_runtime_lib.providers import ProviderManager
from aco_runtime_lib.workflow import OrchestratorOptions, WorkflowOrchestrator

from .api.routes.events import router as events_router
from .api.routes.providers import router as providers_router
from .api.routes.workflow import router as workflow_router
from .api.schemas import HealthResponse


# ── Shared state ──────────────────────────────────────────────────


class AppState:
    """Process-wide singletons shared across requests."""

    def __init__(self) -> None:
        self.bus = EventBus()
        self.manager = ProviderManager()

    async def build_orchestrator(
        self, options: OrchestratorOptions | None = None
    ) -> WorkflowOrchestrator:
        router = self.manager.build_router()
        return WorkflowOrchestrator(self.bus, router, options or OrchestratorOptions())


state = AppState()


# ── Lifecycle ────────────────────────────────────────────────────


@asynccontextmanager
async def lifespan(_app: FastAPI) -> AsyncIterator[None]:
    logger.info("aco-runtime starting up")
    # Eager-init the plugin registry so the first request to
    # /api/plugins doesn't pay the import cost.
    from aco_runtime_lib.plugins.base import get_registry
    plugins = get_registry().list()
    logger.info("plugin registry ready: {}", [p.name for p in plugins])
    # Seed os.environ from the OS keychain BEFORE the provider
    # manager builds its router. Anything the user saved in the
    # Settings UI (or via the secrets CLI) becomes available to the
    # existing code that reads os.environ. The default
    # overwrite=False means an explicit `setx` value beats the
    # keychain value — useful for one-off local overrides.
    try:
        from aco_runtime_lib.secrets import secrets_store
        seeded = secrets_store.seed_os_environ()
        if seeded:
            logger.info("seeded {} env var(s) from keychain: {}", len(seeded), seeded)
    except Exception as exc:  # noqa: BLE001
        # Keyring failure on Linux is common (no Secret Service);
        # log and continue — the user can still setx manually.
        logger.warning("keychain seed skipped: {}", exc)

    providers = state.manager.list_providers()
    enabled = [p.id for p in providers if p.enabled]
    logger.info(
        "provider manager initialized: {} enabled (of {} presets)",
        len(enabled),
        len(providers),
    )
    if enabled:
        logger.info("  enabled providers: {}", enabled)
    else:
        logger.warning(
            "  no providers enabled — set API key env vars to enable them"
        )
    yield
    logger.info("aco-runtime shutting down")


# ── App ──────────────────────────────────────────────────────────


app = FastAPI(
    title="Agent Company OS — Python Runtime",
    version="0.2.3",
    lifespan=lifespan,
)

# CORS: the Vite dev server runs on 127.0.0.1:1420, so we allow
# requests from any localhost origin (browser dev mode only).
app.add_middleware(
    CORSMiddleware,
    allow_origins=[
        "http://127.0.0.1:1420",
        "http://localhost:1420",
        "tauri://localhost",
        "https://tauri.localhost",
    ],
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)


@app.get("/health", response_model=HealthResponse)
async def health() -> HealthResponse:
    """Liveness check; Tauri polls this on startup."""
    return HealthResponse(status="ok", version="0.2.3")


@app.get("/api/state")
async def get_state() -> dict[str, object]:
    """Return the runtime's current state (used by the UI to detect readiness)."""
    return {
        "router_ready": state.manager.has_any_provider(),
        "providers": state.manager.available_providers(),
        "dropped_events": state.bus.dropped_events,
    }


# Wire the shared bus + manager into the routers so the endpoints
# can reach them at request time.
from .api.routes import events as events_module
from .api.routes import plugins as plugins_module
from .api.routes import providers as providers_module
from .api.routes import router as router_module
from .api.routes import settings as settings_module
from .api.routes import workflow as workflow_module

events_module.bind_bus(state.bus)
providers_module.bind_manager(state.manager)
router_module.bind_manager(state.manager)
workflow_module.bind_orchestrator_factory(state.build_orchestrator)

app.include_router(providers_router, prefix="/api/providers", tags=["providers"])
app.include_router(router_module.router, prefix="/api/router", tags=["router"])
app.include_router(workflow_router, prefix="/api/workflow", tags=["workflow"])
app.include_router(events_router, prefix="/api/events", tags=["events"])
app.include_router(settings_module.router, prefix="/api/settings", tags=["settings"])
app.include_router(plugins_module.router, prefix="/api/plugins", tags=["plugins"])


# ── Entry point ──────────────────────────────────────────────────


def main() -> None:
    """Run uvicorn. Defaults to 127.0.0.1:7317 to avoid clashes."""
    host = os.environ.get("ACO_RUNTIME_HOST", "127.0.0.1")
    port = int(os.environ.get("ACO_RUNTIME_PORT", "7317"))

    def _signal_handler(signum: int, _frame: object) -> None:
        logger.info("received signal {}", signum)
        sys.exit(0)

    signal.signal(signal.SIGINT, _signal_handler)
    signal.signal(signal.SIGTERM, _signal_handler)

    uvicorn.run(
        "aco_runtime.main:app",
        host=host,
        port=port,
        log_level="info",
        access_log=False,
        reload=False,
    )


if __name__ == "__main__":
    main()
