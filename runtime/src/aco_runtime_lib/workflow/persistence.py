"""JSONL persistence for the workflow event log.

Every state transition is appended to `$ACO_DATA/workflows/<wf_id>.jsonl`,
one JSON object per line. The file is the source of truth for replay;
the SQLite `workflow_log` table is a derived index.

File format (one line per transition):

```json
{
  "ts": "2026-06-18T12:34:56.789Z",
  "wf_id": "wf_01...",
  "seq": 1,
  "from": null,
  "to": "REQ_RECEIVED",
  "event": "user_input",
  "actor": "agent:user",
  "context": {}
}
```

The runtime `fsync`s after every line so a process kill cannot lose
a committed transition. Writes are **append-only** — no in-place
edits, no rotation. Old workflows are moved aside by the user
(Settings → Archive) or pruned per `aco.toml` retention.

See `docs/STORAGE_SPEC.md` §6.
"""

from __future__ import annotations

import asyncio
import json
import os
import tempfile
from collections.abc import AsyncIterator
from dataclasses import asdict, dataclass
from datetime import UTC, datetime
from pathlib import Path

from aco_runtime_lib.workflow.state_machine import State


@dataclass(slots=True, frozen=True)
class LogEntry:
    """One line in the JSONL workflow log."""

    ts: str
    wf_id: str
    seq: int
    from_state: str | None
    to_state: str
    event: str
    actor: str
    context: dict[str, object]


def now_iso() -> str:
    """ISO 8601 UTC with millisecond precision and `Z` suffix."""
    return datetime.now(UTC).isoformat(timespec="milliseconds").replace("+00:00", "Z")


def workflow_log_path(workflows_dir: Path, wf_id: str) -> Path:
    """Return the canonical path for a workflow's JSONL log."""
    return workflows_dir / f"{wf_id}.jsonl"


def context_for(state: State) -> dict[str, object]:
    """Default context for a transition. Subclass-friendly."""
    return {}


class WorkflowLog:
    """Append-only writer for a single workflow's JSONL log.

    Not safe for concurrent writers to the same workflow — only one
    process may own a workflow at a time. The Tauri shell enforces
    this via the workflow id registry.
    """

    def __init__(self, path: Path) -> None:
        self._path = path
        self._lock = asyncio.Lock()
        self._seq: int = 0
        self._fh: object | None = None  # opened lazily

    @property
    def path(self) -> Path:
        return self._path

    async def open(self) -> None:
        """Open the file (creating parent dirs) and seed the seq counter
        by reading any existing entries.
        """
        async with self._lock:
            self._path.parent.mkdir(parents=True, exist_ok=True)
            if self._path.exists():
                # Count existing lines; cheapest is to count newlines.
                text = self._path.read_text(encoding="utf-8")
                self._seq = text.count("\n")
                # Truncate the file at the end of the last newline in
                # case the previous run crashed mid-write.
                if not text.endswith("\n"):
                    last_nl = text.rfind("\n")
                    if last_nl >= 0:
                        self._path.write_text(text[: last_nl + 1], encoding="utf-8")
            self._fh = self._path.open("a", encoding="utf-8", buffering=1)

    async def close(self) -> None:
        async with self._lock:
            if self._fh is not None:
                close = getattr(self._fh, "close", None)
                if callable(close):
                    close()
                self._fh = None

    async def append(
        self,
        wf_id: str,
        from_state: State | None,
        to_state: State,
        event: str,
        actor: str,
        context: dict[str, object] | None = None,
    ) -> LogEntry:
        """Append one entry and fsync."""
        if self._fh is None:
            raise RuntimeError("WorkflowLog not opened; call open() first")
        async with self._lock:
            self._seq += 1
            entry = LogEntry(
                ts=now_iso(),
                wf_id=wf_id,
                seq=self._seq,
                from_state=from_state.value if from_state is not None else None,
                to_state=to_state.value,
                event=event,
                actor=actor,
                context=context or {},
            )
            line = json.dumps(asdict(entry), separators=(",", ":"), ensure_ascii=False)
            self._fh.write(line + "\n")  # type: ignore[union-attr]
            flush = getattr(self._fh, "flush", None)
            if callable(flush):
                flush()
            # Best-effort fsync for durability. The underlying BufferedWriter
            # doesn't expose fsync directly, so we use os.fsync on the
            # raw file descriptor when available.
            raw = getattr(self._fh, "fileno", None)
            if callable(raw):
                try:
                    os.fsync(raw())
                except OSError:
                    pass
            return entry


async def iter_entries(path: Path) -> AsyncIterator[LogEntry]:
    """Yield every entry in a JSONL log file. Malformed lines are skipped."""
    if not path.exists():
        return
    with path.open("r", encoding="utf-8") as f:
        for raw in f:
            raw = raw.strip()
            if not raw:
                continue
            try:
                obj = json.loads(raw)
            except json.JSONDecodeError:
                continue
            try:
                yield LogEntry(
                    ts=obj["ts"],
                    wf_id=obj["wf_id"],
                    seq=obj["seq"],
                    from_state=obj.get("from_state"),
                    to_state=obj["to_state"],
                    event=obj["event"],
                    actor=obj["actor"],
                    context=obj.get("context", {}),
                )
            except KeyError:
                continue


def last_entry(path: Path) -> LogEntry | None:
    """Return the last entry in a log, or None if empty/missing."""
    last: LogEntry | None = None
    for entry in iter_entries_sync(path):
        last = entry
    return last


def iter_entries_sync(path: Path):  # type: ignore[no-untyped-def]
    """Synchronous iterator over a JSONL log (for tests + recovery)."""
    if not path.exists():
        return
    with path.open("r", encoding="utf-8") as f:
        for raw in f:
            raw = raw.strip()
            if not raw:
                continue
            try:
                obj = json.loads(raw)
            except json.JSONDecodeError:
                continue
            try:
                yield LogEntry(
                    ts=obj["ts"],
                    wf_id=obj["wf_id"],
                    seq=obj["seq"],
                    from_state=obj.get("from_state"),
                    to_state=obj["to_state"],
                    event=obj["event"],
                    actor=obj["actor"],
                    context=obj.get("context", {}),
                )
            except KeyError:
                continue


# `_async_iter` adapter used by `iter_entries`
async def _aiter_sync(it: object) -> AsyncIterator[LogEntry]:
    for entry in it:  # type: ignore[union-attr]
        yield entry


# Re-export under the async name
async def iter_entries_async(path: Path) -> AsyncIterator[LogEntry]:
    async for entry in _aiter_sync(iter_entries_sync(path)):
        yield entry


# Patch the alias: `iter_entries` returns the async-friendly form
# (the name conflict is intentional — callers use `async for`).
iter_entries = iter_entries_async


# ── Atomic write helper (for tests + first-time writes) ─────────


def atomic_write_text(path: Path, text: str) -> None:
    """Write `text` to `path` atomically (tempfile + rename)."""
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(
        prefix=f".{path.name}.",
        suffix=".tmp",
        dir=str(path.parent),
    )
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as f:
            f.write(text)
            f.flush()
            os.fsync(f.fileno())
        os.replace(tmp, path)
    except Exception:
        try:
            os.unlink(tmp)
        except OSError:
            pass
        raise
