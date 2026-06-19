"""Tests for the structured plugin system (Phase 2 / C).

Covers:
* Registry register / unregister / list / invoke
* Echo plugin round-trip
* Python plugin (inline source + timeout)
* Git plugin (read-only + confirm gating for write ops)
* Error normalization (unknown plugin, plugin exception, non-dict return)
"""
from __future__ import annotations

import asyncio
import os

import pytest

from aco_runtime_lib.plugins.base import Plugin, PluginRegistry, get_registry


# ── EchoPlugin ──────────────────────────────────────────────────


def test_echo_plugin_roundtrip() -> None:
    reg = PluginRegistry()
    from aco_runtime_lib.plugins.builtin.echo import EchoPlugin
    reg.register(EchoPlugin())
    result = asyncio.run(reg.invoke("echo", {"hello": "world"}))
    assert result["status"] == "ok"
    assert result["echoed"] == {"hello": "world"}


# ── PythonPlugin ────────────────────────────────────────────────


def test_python_inline_source_runs() -> None:
    reg = PluginRegistry()
    from aco_runtime_lib.plugins.builtin.python import PythonPlugin
    reg.register(PythonPlugin())
    result = asyncio.run(
        reg.invoke("python", {"code": "print(2 + 2)"})
    )
    assert result["status"] == "ok"
    assert result["exit_code"] == 0
    assert "4" in result["stdout"]


def test_python_requires_code_or_script() -> None:
    reg = PluginRegistry()
    from aco_runtime_lib.plugins.builtin.python import PythonPlugin
    reg.register(PythonPlugin())
    result = asyncio.run(reg.invoke("python", {}))
    assert result["status"] == "error"
    assert "code" in result["message"] or "script" in result["message"]


def test_python_rejects_both_code_and_script() -> None:
    reg = PluginRegistry()
    from aco_runtime_lib.plugins.builtin.python import PythonPlugin
    reg.register(PythonPlugin())
    result = asyncio.run(
        reg.invoke("python", {"code": "1", "script": "x.py"})
    )
    assert result["status"] == "error"


def test_python_timeout_returns_error() -> None:
    reg = PluginRegistry()
    from aco_runtime_lib.plugins.builtin.python import PythonPlugin
    reg.register(PythonPlugin())
    # Long-running sleep, very short timeout.
    result = asyncio.run(
        reg.invoke(
            "python",
            {"code": "import time; time.sleep(10)", "timeout_seconds": 0.5},
        )
    )
    assert result["status"] == "error"
    assert "timeout" in result["message"].lower()


# ── GitPlugin ───────────────────────────────────────────────────


def test_git_read_only_runs() -> None:
    reg = PluginRegistry()
    from aco_runtime_lib.plugins.builtin.git import GitPlugin
    reg.register(GitPlugin())
    result = asyncio.run(
        reg.invoke(
            "git",
            {"args": ["rev-parse", "--show-toplevel"]},
        )
    )
    assert result["status"] == "ok"
    assert result["write"] is False
    # stdout should be a directory
    assert os.path.isdir(result["stdout"].strip())


def test_git_write_requires_confirm() -> None:
    reg = PluginRegistry()
    from aco_runtime_lib.plugins.builtin.git import GitPlugin
    reg.register(GitPlugin())
    result = asyncio.run(reg.invoke("git", {"args": ["commit", "-m", "x"]}))
    assert result["status"] == "error"
    assert "confirm" in result["message"].lower()


def test_git_missing_args() -> None:
    reg = PluginRegistry()
    from aco_runtime_lib.plugins.builtin.git import GitPlugin
    reg.register(GitPlugin())
    result = asyncio.run(reg.invoke("git", {}))
    assert result["status"] == "error"
    assert "args" in result["message"]


# ── Registry behavior ───────────────────────────────────────────


def test_registry_unknown_plugin() -> None:
    reg = PluginRegistry()
    result = asyncio.run(reg.invoke("does-not-exist"))
    assert result["status"] == "error"
    assert "available" in result
    assert result["available"] == []


def test_registry_rejects_duplicate() -> None:
    reg = PluginRegistry()
    from aco_runtime_lib.plugins.builtin.echo import EchoPlugin
    reg.register(EchoPlugin())
    with pytest.raises(ValueError):
        reg.register(EchoPlugin())


def test_registry_rejects_empty_name() -> None:
    reg = PluginRegistry()

    class _NoName(Plugin):
        name = ""

        async def invoke(self, args, ctx=None):
            return {}

    with pytest.raises(ValueError):
        reg.register(_NoName())


def test_registry_normalizes_non_dict_return() -> None:
    """A buggy plugin returning a list should be wrapped, not
    propagated."""

    class _Bad(Plugin):
        name = "bad"

        async def invoke(self, args, ctx=None):
            return ["not", "a", "dict"]  # type: ignore[return-value]

    reg = PluginRegistry()
    reg.register(_Bad())
    result = asyncio.run(reg.invoke("bad"))
    assert result["status"] == "error"
    assert "non-dict" in result["message"]


def test_registry_catches_plugin_exception() -> None:
    class _Boom(Plugin):
        name = "boom"

        async def invoke(self, args, ctx=None):
            raise RuntimeError("kaboom")

    reg = PluginRegistry()
    reg.register(_Boom())
    result = asyncio.run(reg.invoke("boom"))
    assert result["status"] == "error"
    assert "kaboom" in result["message"]


def test_get_registry_singleton_has_builtins() -> None:
    reg = get_registry()
    names = {p.name for p in reg.list()}
    assert "echo" in names
    assert "python" in names
    assert "git" in names
