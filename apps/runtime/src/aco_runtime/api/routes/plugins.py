"""Plugin REST API.

GET    /api/plugins                       list available plugins
GET    /api/plugins/{name}                plugin descriptor
POST   /api/plugins/{name}/invoke         invoke with JSON body

The invoke endpoint takes whatever args the plugin's invoke()
expects. For example::

    POST /api/plugins/python/invoke
    {"args": {"code": "print(2+2)"}}

    POST /api/plugins/git/invoke
    {"args": {"args": ["status", "--short"], "cwd": "..."}}
"""
from __future__ import annotations

from typing import Any

from fastapi import APIRouter, HTTPException

from aco_runtime_lib.plugins.base import get_registry

router = APIRouter()


def _reg() -> Any:
    return get_registry()


@router.get("")
def list_plugins() -> list[dict[str, Any]]:
    return [
        {"name": d.name, "description": d.description, "actions": d.actions}
        for d in _reg().list()
    ]


@router.get("/{name}")
def get_plugin(name: str) -> dict[str, Any]:
    p = _reg().get(name)
    if p is None:
        raise HTTPException(status_code=404, detail="plugin not found")
    return {
        "name": p.name,
        "description": p.description,
        "actions": getattr(p, "actions", None) or ["*"],
    }


@router.post("/{name}/invoke")
async def invoke_plugin(name: str, body: dict[str, Any]) -> dict[str, Any]:
    args = body.get("args") or {}
    ctx = body.get("ctx") or None
    return await _reg().invoke(name, args, ctx)
