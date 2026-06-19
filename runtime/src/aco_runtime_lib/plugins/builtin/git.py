"""Git plugin — run git subcommands safely.

Supports the common read-only and repo-info commands:
  * status, log, diff, show, branch, tag, remote, rev-parse
  * ls-files, ls-tree, cat-file
  * add, commit, push, pull, fetch (write ops; require confirmation in UI)

Args:
  {"args": ["status", "--short"]}   # args list (positional + flags)
  {"cwd": "/path/to/repo"}          # default: os.getcwd()
  {"timeout_seconds": 30}           # default 30

Returns stdout, stderr, exit_code.
"""
from __future__ import annotations

import asyncio
import os
import shutil
from collections.abc import Mapping
from typing import Any

from aco_runtime_lib.plugins.base import Plugin


_DEFAULT_TIMEOUT = 30.0

# Commands considered "safe" (read-only). The plugin runs these
# without an extra confirmation step. Write operations require
# the caller to pass confirm=true.
_READ_ONLY = frozenset({
    "status", "log", "diff", "show", "branch", "tag", "remote",
    "rev-parse", "ls-files", "ls-tree", "cat-file", "config",
    "describe", "log", "shortlog", "stash", "tag", "blame",
    "reflog",
})


class GitPlugin(Plugin):
    name = "git"
    description = (
        "Run git subcommands in a subprocess. Args: "
        "{\"args\": [\"status\", \"--short\"], \"cwd\": \"/repo\"}. "
        "Read-only commands (status, log, diff, ...) work without "
        "confirmation. Write commands (commit, push, ...) require "
        "confirm=true."
    )
    actions = ["exec"] + sorted(_READ_ONLY) + ["commit", "push", "pull", "fetch", "add"]

    async def invoke(
        self, args: Mapping[str, Any], ctx: Mapping[str, Any] | None = None
    ) -> dict[str, Any]:
        cmd_args = args.get("args")
        if not cmd_args or not isinstance(cmd_args, list):
            return {
                "status": "error",
                "message": "provide 'args' as a non-empty list",
            }
        subcommand = cmd_args[0]
        is_write = subcommand not in _READ_ONLY
        if is_write and not args.get("confirm"):
            return {
                "status": "error",
                "message": (
                    f"git {subcommand} is a write operation; "
                    f"pass confirm=true to proceed"
                ),
            }

        cwd = args.get("cwd") or os.getcwd()
        timeout = float(args.get("timeout_seconds", _DEFAULT_TIMEOUT))

        git = shutil.which("git")
        if git is None:
            return {"status": "error", "message": "git not found on PATH"}

        full_cmd = [git, *cmd_args]
        try:
            proc = await asyncio.create_subprocess_exec(
                *full_cmd,
                cwd=cwd,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
            )
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
            "write": is_write,
        }
