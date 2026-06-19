"""Echo plugin — sanity-test the plugin pipeline.

Returns whatever args you pass. Useful for debugging the
Worker -> plugin handoff.
"""
from __future__ import annotations

from collections.abc import Mapping
from typing import Any

from aco_runtime_lib.plugins.base import Plugin


class EchoPlugin(Plugin):
    name = "echo"
    description = "Returns its args unchanged. Useful for smoke-testing the plugin pipeline."
    actions = ["echo"]

    async def invoke(
        self, args: Mapping[str, Any], ctx: Mapping[str, Any] | None = None
    ) -> dict[str, Any]:
        return {
            "status": "ok",
            "echoed": dict(args),
            "ctx_keys": list((ctx or {}).keys()),
        }
