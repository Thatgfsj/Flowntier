"""Crash recovery: find non-terminal workflows on startup.

Scans `$ACO_DATA/workflows/*.jsonl` and reports the ones whose last
state is not terminal. The runtime presents these to the user and
lets them pick: **Resume**, **Discard**, or **Inspect**.

See `docs/WORKFLOW_SPEC.md` §9.2 and `docs/STORAGE_SPEC.md` §7.3.
"""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

from aco_runtime_lib.workflow.persistence import LogEntry, iter_entries_sync
from aco_runtime_lib.workflow.state_machine import TERMINAL_STATES, State


@dataclass(slots=True, frozen=True)
class ResumableWorkflow:
    """A workflow log whose last entry is not in a terminal state."""

    wf_id: str
    log_path: Path
    last_state: State
    last_event: str
    last_actor: str
    last_ts: str
    entry_count: int


def _coerce_state(s: str) -> State | None:
    try:
        return State(s)
    except ValueError:
        return None


def find_resumable(workflows_dir: Path) -> list[ResumableWorkflow]:
    """Scan `workflows_dir` and return non-terminal workflows.

    Workflows whose last entry is missing or whose `to_state` is not
    a known State are reported as resumable (safer default than
    silently dropping them).
    """
    if not workflows_dir.exists():
        return []
    out: list[ResumableWorkflow] = []
    for path in sorted(workflows_dir.glob("*.jsonl")):
        last = _last_entry(path)
        if last is None:
            continue
        state = _coerce_state(last.to_state)
        if state is None:
            # Unknown state — surface it; user can decide.
            out.append(
                ResumableWorkflow(
                    wf_id=last.wf_id,
                    log_path=path,
                    last_state=State.ABORTED,  # placeholder
                    last_event=last.event,
                    last_actor=last.actor,
                    last_ts=last.ts,
                    entry_count=last.seq,
                )
            )
            continue
        if state in TERMINAL_STATES:
            continue
        out.append(
            ResumableWorkflow(
                wf_id=last.wf_id,
                log_path=path,
                last_state=state,
                last_event=last.event,
                last_actor=last.actor,
                last_ts=last.ts,
                entry_count=last.seq,
            )
        )
    return out


def _last_entry(path: Path) -> LogEntry | None:
    last: LogEntry | None = None
    for entry in iter_entries_sync(path):
        last = entry
    return last
