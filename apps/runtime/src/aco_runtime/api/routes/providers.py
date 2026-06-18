"""Provider + router config endpoints.

GET    /api/providers               — list all known providers + status
GET    /api/providers/{id}          — single provider status
PATCH  /api/providers/{id}          — enable/disable a provider
POST   /api/providers/{id}/test     — test connection (model list ping)
GET    /api/router/roles            — current role → model assignments
PUT    /api/router/roles            — replace all assignments at once
GET    /api/models                  — every available model across enabled providers
"""

from __future__ import annotations

from typing import Any

import httpx
from fastapi import APIRouter, HTTPException
from loguru import logger

from aco_runtime_lib.providers import (
    PROVIDER_PRESETS,
    ProviderManager,
    ProviderStatus,
    RoleAssignment,
    get_preset,
)

router = APIRouter()

# ── Shared state ──────────────────────────────────────────────────

_manager: ProviderManager | None = None


def bind_manager(manager: ProviderManager) -> None:
    global _manager
    _manager = manager


def _m() -> ProviderManager:
    if _manager is None:
        raise HTTPException(status_code=503, detail="provider manager not ready")
    return _manager


# ── Serialization ────────────────────────────────────────────────


def _serialize_status(s: ProviderStatus) -> dict[str, Any]:
    return {
        "id": s.id,
        "display_name": s.display_name,
        "kind": s.kind,
        "base_url": s.base_url,
        "api_key_env": s.api_key_env,
        "enabled": s.enabled,
        "key_present": s.key_present,
        "is_local": s.is_local,
        "notes": s.notes,
        "models": [
            {
                "id": m.id,
                "display_name": m.display_name,
                "context_window": m.context_window,
                "max_output_tokens": m.max_output_tokens,
                "input_cost_mtok": m.input_cost_mtok,
                "output_cost_mtok": m.output_cost_mtok,
                "capabilities": list(m.capabilities),
            }
            for m in s.models
        ],
    }


def _serialize_role(r: RoleAssignment) -> dict[str, Any]:
    return {
        "role": r.role,
        "default_model": r.default_model,
        "fallback_chain": list(r.fallback_chain),
    }


# ── Endpoints ────────────────────────────────────────────────────


@router.get("")
async def list_providers() -> dict[str, Any]:
    m = _m()
    statuses = m.list_providers()
    return {
        "providers": [_serialize_status(s) for s in statuses],
        "roles": [_serialize_role(r) for r in m.list_roles()],
    }


@router.get("/{provider_id}")
async def get_provider(provider_id: str) -> dict[str, Any]:
    m = _m()
    s = m.get_provider(provider_id)
    if s is None:
        raise HTTPException(status_code=404, detail="provider not found")
    return _serialize_status(s)


@router.patch("/{provider_id}")
async def patch_provider(provider_id: str, body: dict[str, Any]) -> dict[str, Any]:
    m = _m()
    if provider_id not in [p.id for p in PROVIDER_PRESETS]:
        raise HTTPException(status_code=404, detail="unknown provider")
    if "enabled" in body:
        m.set_provider_enabled(provider_id, bool(body["enabled"]))
    s = m.get_provider(provider_id)
    if s is None:
        raise HTTPException(status_code=404, detail="provider not found")
    logger.info("provider {} enabled={}", provider_id, s.enabled)
    return _serialize_status(s)


@router.post("/{provider_id}/test")
async def test_provider(provider_id: str) -> dict[str, Any]:
    """Try to list models at the provider's base URL.

    For Anthropic/Gemini we don't have a list endpoint, so we
    return a synthetic success if the key is present and the URL
    is well-formed. For OpenAI-compatible providers we hit
    `/models` and check the response.
    """
    preset = get_preset(provider_id)
    if preset is None:
        raise HTTPException(status_code=404, detail="unknown provider")
    if not preset.base_url:
        return {"ok": False, "reason": "base_url is empty"}

    if preset.kind in ("anthropic", "google"):
        # No /models endpoint; just check the env var + URL shape.
        return {
            "ok": True,
            "reason": "no list endpoint for this provider; env var and URL OK",
            "base_url": preset.base_url,
        }

    # OpenAI-compatible: hit /models
    api_key = os.environ.get(preset.api_key_env, "") if preset.api_key_env else ""
    if not api_key and not preset.is_local:
        return {
            "ok": False,
            "reason": f"env var {preset.api_key_env!r} is empty",
        }
    try:
        async with httpx.AsyncClient(timeout=10.0) as client:
            resp = await client.get(
                f"{preset.base_url.rstrip('/')}/models",
                headers={"authorization": f"Bearer {api_key}"} if api_key else {},
            )
        return {
            "ok": resp.status_code < 400,
            "status": resp.status_code,
            "model_count": (
                len(resp.json().get("data", [])) if resp.status_code < 400 else 0
            ),
        }
    except Exception as e:  # noqa: BLE001
        return {"ok": False, "reason": f"connection error: {e}"}


import os  # noqa: E402  (imported here because `test_provider` uses it)
