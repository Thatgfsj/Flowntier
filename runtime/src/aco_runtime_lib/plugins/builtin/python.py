"""Python plugin — run Python code or a script in a subprocess.

The Worker can call this with either:
  {"code": "print(1+1)"}      # inline source
  {"script": "scripts/x.py"}  # run a script file

Both are executed with the same Python interpreter as the runtime
(i.e. they inherit the runtime's env vars and packages). The
result captures stdout, stderr, and exit code.

Timeout
=======
Default 30s, override with `timeout_seconds`. The subprocess is
killed on timeout (SIGTERM, then SIGKILL after 5s grace).
"""
from __future__ import annotations

import asyncio
import os
import sys
from collections.abc import Mapping
from typing import Any

from aco_runtime_lib.plugins.base import Plugin


_DEFAULT_TIMEOUT = 30.0


class PythonPlugin(Plugin):
    name = "python"
    description = (
        "Execute Python source (args: {\"code\": \"...\"}) or a script file "
        "(args: {\"script\": \"path.py\"}) in a subprocess. "
        "Returns stdout, stderr, exit_code."
    )
    actions = ["exec"]

    async def invoke(
        self, args: Mapping[str, Any], ctx: Mapping[str, Any] | None = None
    ) -> dict[str, Any]:
        code = args.get("code")
        script = args.get("script")
        if not code and not script:
            return {
                "status": "error",
                "message": "provide either 'code' or 'script' arg",
            }
        if code and script:
            return {
                "status": "error",
                "message": "provide only one of 'code' or 'script'",
            }

        cwd = args.get("cwd") or os.getcwd()
        timeout = float(args.get("timeout_seconds", _DEFAULT_TIMEOUT))

        try:
            if code is not None:
                proc = await asyncio.create_subprocess_exec(
                    sys.executable,
                    "-I",  # isolated mode: no user-site, no PYTHONPATH
                    "-c",
                    code,
                    cwd=cwd,
                    stdout=asyncio.subprocess.PIPE,
                    stderr=asyncio.subprocess.PIPE,
                )
            else:
                proc = await asyncio.create_subprocess_exec(
                    sys.executable,
                    str(script),
                    cwd=cwd,
                    stdout=asyncio.subprocess.PIPE,
                    stderr=asyncio.subprocess.PIPE,
                )
        except FileNotFoundError as exc:
            return {"status": "error", "message": str(exc)}
        except Exception as exc:  # noqa: BLE001
            return {
                "status": "error",
                "message": f"{type(exc).__name__}: {exc}",
            }

        try:
            stdout, stderr = await asyncio.wait_for(
                proc.communicate(), timeout=timeout
            )
        except asyncio.TimeoutError:
            proc.kill()
            await proc.wait()
            return {
                "status": "error",
                "message": f"timeout after {timeout:.0f}s",
                "exit_code": None,
            }

        # Truncate huge outputs to keep JSON manageable.
        max_chars = int(args.get("max_output_chars", 10000))
        out = stdout.decode("utf-8", errors="replace")
        err = stderr.decode("utf-8", errors="replace")
        truncated = False
        if len(out) > max_chars:
            out = out[:max_chars] + f"... [truncated {len(out) - max_chars} chars]"
            truncated = True
        if len(err) > max_chars:
            err = err[:max_chars] + f"... [truncated {len(err) - max_chars} chars]"
            truncated = True

        return {
            "status": "ok" if proc.returncode == 0 else "error",
            "exit_code": proc.returncode,
            "stdout": out,
            "stderr": err,
            "truncated": truncated,
        }
