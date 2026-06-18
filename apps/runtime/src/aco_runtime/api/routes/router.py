"""Router / role assignment endpoints (mounted at /api/router)."""

from __future__ import annotations

from typing import Any

from fastapi import APIRouter, HTTPException
from loguru import logger

from aco_runtime_lib.providers import (
    ProviderManager,
    RoleAssignment,
)

router = APIRouter()

_manager: ProviderManager | None = None


def bind_manager(manager: ProviderManager) -> None:
    global _manager
    _manager = manager


def _m() -> ProviderManager:
    if _manager is None:
        raise HTTPException(status_code=503, detail="provider manager not ready")
    return _manager


def _serialize(r: RoleAssignment) -> dict[str, Any]:
    return {
        "role": r.role,
        "default_model": r.default_model,
        "fallback_chain": list(r.fallback_chain),
    }


@router.get("/roles")
async def get_roles() -> dict[str, Any]:
    return {"roles": [_serialize(r) for r in _m().list_roles()]}


@router.put("/roles")
async def put_roles(body: dict[str, Any]) -> dict[str, Any]:
    m = _m()
    roles = body.get("roles")
    if not isinstance(roles, list):
        raise HTTPException(status_code=400, detail="`roles` must be a list")
    for entry in roles:
        role = entry.get("role")
        default = entry.get("default_model")
        chain = entry.get("fallback_chain", [])
        if not role or not default:
            raise HTTPException(
                status_code=400,
                detail=f"role entry missing role/default_model: {entry}",
            )
        try:
            m.set_role_default(role, default)
            m.set_fallback_chain(role, chain)
        except (KeyError, ValueError) as e:
            raise HTTPException(status_code=400, detail=str(e))
    logger.info("router roles updated")
    return {"roles": [_serialize(r) for r in m.list_roles()]}


@router.get("/models")
async def list_models() -> dict[str, Any]:
    m = _m()
    out: list[dict[str, Any]] = []
    for s in m.list_providers():
        if not s.enabled:
            continue
        for model in s.models:
            out.append(
                {
                    "provider": s.id,
                    "provider_display": s.display_name,
                    "model": model.id,
                    "display_name": model.display_name,
                }
            )
    return {"models": out}
