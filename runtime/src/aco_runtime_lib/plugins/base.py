"""Structured plugin system.

A Plugin is anything the Worker can call to do work outside of
the LLM: run a shell command, invoke Python, talk to git, hit
an HTTP endpoint, etc. The plugin framework:

* Defines a uniform `Plugin` ABC with `invoke(args, ctx) -> dict`.
* Provides a `PluginRegistry` that owns the set of available
  plugins and dispatches calls.
* Ships a few built-in plugins (python, git, echo) that the
  Worker can call by name.

Why this exists
===============
The user wants Worker agents to call external tools (python,
git) without resorting to free-form shell. Free-form shell is a
safety hazard with local small models — a misfire could
trash the user's repo. A structured plugin surface gives the
LLM a typed vocabulary:

    {"plugin": "git", "action": "status", "args": {}}

The plugin implementation handles subprocess invocation,
timeouts, and result formatting. The Worker just emits JSON.

Plugin discovery
===============
Built-in plugins live in `aco_runtime_lib/plugins/builtin/`.
External plugins (Phase 2.13+) will be loaded from a directory
specified in the runtime config (out of scope for now).

Calling convention
=================
    reg = PluginRegistry()
    reg.register(PythonPlugin())
    reg.register(GitPlugin())
    result = await reg.invoke("python", {"code": "print(1+1)"})
    # {"status": "ok", "stdout": "2
", "stderr": "", "exit_code": 0}
"""
from __future__ import annotations

import asyncio
import inspect
from abc import ABC, abstractmethod
from collections.abc import Mapping
from dataclasses import dataclass, field
from typing import Any


class Plugin(ABC):
    """Base class for all plugins.

    Subclasses must set `name` (unique) and implement `invoke`.
    """

    name: str = ""
    description: str = ""
    """Human-readable description shown in /api/plugins."""

    @abstractmethod
    async def invoke(
        self, args: Mapping[str, Any], ctx: Mapping[str, Any] | None = None
    ) -> dict[str, Any]:
        """Run the plugin with the given args.

        Returns a JSON-serializable dict. Plugins should never raise
        on user input; instead return `{"status": "error",
        "message": str(exc)}` so the Worker can surface the failure
        in its LLM prompt.
        """


@dataclass(slots=True)
class PluginDescriptor:
    """What the registry exposes to the UI / Worker."""
    name: str
    description: str
    actions: list[str] = field(default_factory=list)
    """List of supported action names (or ["*"] if arbitrary)."""


class PluginRegistry:
    """Owns the set of registered plugins and dispatches calls."""

    def __init__(self) -> None:
        self._plugins: dict[str, Plugin] = {}

    def register(self, plugin: Plugin) -> None:
        if not plugin.name:
            raise ValueError(f"{type(plugin).__name__}.name is empty")
        if plugin.name in self._plugins:
            raise ValueError(f"plugin {plugin.name!r} already registered")
        self._plugins[plugin.name] = plugin

    def unregister(self, name: str) -> None:
        self._plugins.pop(name, None)

    def list(self) -> list[PluginDescriptor]:
        out: list[PluginDescriptor] = []
        for p in self._plugins.values():
            actions = getattr(p, "actions", None) or ["*"]
            out.append(PluginDescriptor(
                name=p.name, description=p.description, actions=list(actions)
            ))
        return out

    def get(self, name: str) -> Plugin | None:
        return self._plugins.get(name)

    async def invoke(
        self,
        name: str,
        args: Mapping[str, Any] | None = None,
        ctx: Mapping[str, Any] | None = None,
    ) -> dict[str, Any]:
        plugin = self._plugins.get(name)
        if plugin is None:
            return {
                "status": "error",
                "message": f"unknown plugin: {name!r}",
                "available": [p.name for p in self._plugins.values()],
            }
        try:
            result = await plugin.invoke(args or {}, ctx or {})
            # Validate result is dict[str, Any]
            if not isinstance(result, dict):
                return {
                    "status": "error",
                    "message": (
                        f"plugin {name!r} returned non-dict: "
                        f"{type(result).__name__}"
                    ),
                }
            # Normalize: ensure 'status' is set
            result.setdefault("status", "ok")
            return result
        except Exception as exc:  # noqa: BLE001
            return {
                "status": "error",
                "message": f"{type(exc).__name__}: {exc}",
            }


# ── Module-level singleton ──────────────────────────────────────

_registry: PluginRegistry | None = None


def get_registry(work_dir: str = ".") -> PluginRegistry:
    """Lazily build the registry with built-in plugins loaded."""
    global _registry
    if _registry is None:
        from aco_runtime_lib.plugins.builtin.docker import DockerPlugin
        from aco_runtime_lib.plugins.builtin.echo import EchoPlugin
        from aco_runtime_lib.plugins.builtin.file_ops import FileOpsPlugin
        from aco_runtime_lib.plugins.builtin.git import GitPlugin
        from aco_runtime_lib.plugins.builtin.mcp import MCPPlugin
        from aco_runtime_lib.plugins.builtin.python import PythonPlugin

        _registry = PluginRegistry()
        _registry.register(EchoPlugin())
        _registry.register(PythonPlugin())
        _registry.register(FileOpsPlugin(work_dir))
        _registry.register(GitPlugin())
        _registry.register(DockerPlugin())
        _registry.register(MCPPlugin())
    return _registry


__all__ = [
    "Plugin",
    "PluginDescriptor",
    "PluginRegistry",
    "get_registry",
]
