"""File operations plugin for ACO runtime.

Provides file read/write operations for the Worker agent.
Safety: write operations are sandboxed to a working directory.

Spec: `docs/PLUGIN_SPEC.md` §3.4.
"""

from __future__ import annotations

import os
from collections.abc import Mapping
from pathlib import Path
from typing import Any

from aco_runtime_lib.plugins.base import Plugin


class FileOpsPlugin(Plugin):
    """File operations plugin."""

    name = "file_ops"
    description = "File read/write operations"
    actions = ["read_file", "write_file", "list_dir", "exists", "mkdir"]

    def __init__(self, work_dir: str = ".") -> None:
        self._work_dir = os.path.abspath(work_dir)

    def _safe_path(self, path: str) -> str:
        """Ensure path is within working directory."""
        abs_path = os.path.abspath(os.path.join(self._work_dir, path))
        if not abs_path.startswith(self._work_dir):
            raise ValueError(f"Path {path} is outside working directory")
        return abs_path

    async def invoke(
        self, args: Mapping[str, Any], ctx: Mapping[str, Any] | None = None
    ) -> dict[str, Any]:
        action = args.get("action", "")
        if not action:
            # Try to infer action from args
            if "content" in args and "path" in args:
                action = "write_file"
            elif "path" in args and "content" not in args:
                action = "read_file"
            else:
                return {"status": "error", "message": "missing 'action' parameter"}

        handler = getattr(self, f"_action_{action}", None)
        if handler is None:
            return {
                "status": "error",
                "message": f"unknown action: {action!r}",
                "available": self.actions,
            }

        try:
            return await handler(args, ctx or {})
        except Exception as exc:
            return {"status": "error", "message": f"{type(exc).__name__}: {exc}"}

    async def _action_read_file(
        self, args: Mapping[str, Any], ctx: Mapping[str, Any]
    ) -> dict[str, Any]:
        """Read a file."""
        path = args.get("path", "")
        if not path:
            return {"status": "error", "message": "missing 'path' parameter"}

        try:
            safe_path = self._safe_path(path)
            with open(safe_path, "r", encoding="utf-8") as f:
                content = f.read()
            return {
                "status": "ok",
                "path": path,
                "content": content,
                "lines": len(content.split("\n")),
            }
        except FileNotFoundError:
            return {"status": "error", "message": f"file not found: {path}"}
        except Exception as exc:
            return {"status": "error", "message": str(exc)}

    async def _action_write_file(
        self, args: Mapping[str, Any], ctx: Mapping[str, Any]
    ) -> dict[str, Any]:
        """Write content to a file."""
        path = args.get("path", "")
        content = args.get("content", "")

        if not path:
            return {"status": "error", "message": "missing 'path' parameter"}
        if content is None:
            return {"status": "error", "message": "missing 'content' parameter"}

        try:
            safe_path = self._safe_path(path)
            # Create parent directories if needed
            os.makedirs(os.path.dirname(safe_path), exist_ok=True)

            # Check if file exists for lines_removed count
            lines_removed = 0
            if os.path.exists(safe_path):
                with open(safe_path, "r", encoding="utf-8") as f:
                    lines_removed = len(f.readlines())

            # Write the file
            with open(safe_path, "w", encoding="utf-8") as f:
                f.write(content)

            lines_added = len(content.split("\n"))
            return {
                "status": "ok",
                "path": path,
                "lines_added": lines_added,
                "lines_removed": lines_removed,
                "bytes_written": len(content.encode("utf-8")),
            }
        except Exception as exc:
            return {"status": "error", "message": str(exc)}

    async def _action_list_dir(
        self, args: Mapping[str, Any], ctx: Mapping[str, Any]
    ) -> dict[str, Any]:
        """List directory contents."""
        path = args.get("path", ".")

        try:
            safe_path = self._safe_path(path)
            entries = []
            for entry in os.scandir(safe_path):
                entries.append({
                    "name": entry.name,
                    "type": "dir" if entry.is_dir() else "file",
                    "size": entry.stat().st_size if entry.is_file() else None,
                })
            return {
                "status": "ok",
                "path": path,
                "entries": entries,
                "count": len(entries),
            }
        except Exception as exc:
            return {"status": "error", "message": str(exc)}

    async def _action_exists(
        self, args: Mapping[str, Any], ctx: Mapping[str, Any]
    ) -> dict[str, Any]:
        """Check if file/directory exists."""
        path = args.get("path", "")
        if not path:
            return {"status": "error", "message": "missing 'path' parameter"}

        try:
            safe_path = self._safe_path(path)
            return {
                "status": "ok",
                "path": path,
                "exists": os.path.exists(safe_path),
                "is_file": os.path.isfile(safe_path),
                "is_dir": os.path.isdir(safe_path),
            }
        except Exception as exc:
            return {"status": "error", "message": str(exc)}

    async def _action_mkdir(
        self, args: Mapping[str, Any], ctx: Mapping[str, Any]
    ) -> dict[str, Any]:
        """Create directory."""
        path = args.get("path", "")
        if not path:
            return {"status": "error", "message": "missing 'path' parameter"}

        try:
            safe_path = self._safe_path(path)
            os.makedirs(safe_path, exist_ok=True)
            return {
                "status": "ok",
                "path": path,
                "created": True,
            }
        except Exception as exc:
            return {"status": "error", "message": str(exc)}
