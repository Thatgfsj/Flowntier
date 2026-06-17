"""Reporter agent — composes the final user-facing summary.

Phase 1: derives the summary from the workflow log + final task
results. No LLM call needed if the data is already structured.
A small LLM call may be used in Phase 2 to polish prose.

See `prompts/reporter.md` and `docs/AGENT_PROTOCOL.md` §3.
"""

from __future__ import annotations

from collections.abc import Iterable
from typing import Any

from aco_runtime_lib.agents.base import Agent, AgentResult, AgentRole
from aco_runtime_lib.workflow.persistence import LogEntry


class ReporterAgent(Agent):
    role = AgentRole.REPORTER

    async def run(self, ctx: dict[str, Any]) -> AgentResult:
        log: Iterable[LogEntry] = ctx.get("log", [])
        task_results: list[dict[str, Any]] = ctx.get("task_results", [])
        workflow_status: str = ctx.get("workflow_status", "DONE")
        summary = _compose_summary(list(log), task_results, workflow_status)
        return AgentResult(role=self.role, data={"summary": summary})


def _compose_summary(
    log: list[LogEntry],
    task_results: list[dict[str, Any]],
    workflow_status: str,
) -> str:
    """Compose a deterministic Markdown summary.

    Phase 1: a templated summary. Phase 2 may call the provider to
    polish prose.
    """
    if workflow_status != "DONE":
        return _compose_failure_summary(workflow_status, task_results)

    completed = [r for r in task_results if r.get("status") == "DONE"]
    files: set[str] = set()
    for r in completed:
        for f in r.get("files_modified", []):
            files.add(f["path"])

    sections: list[str] = []
    sections.append("# Delivery Summary")
    sections.append("")
    sections.append("## What was built")
    for r in completed:
        sections.append(f"- {r.get('summary', '(no summary)')}")
    if not completed:
        sections.append("- (no completed tasks)")
    sections.append("")
    sections.append("## Files modified")
    if files:
        sections.append("```")
        for f in sorted(files):
            sections.append(f)
        sections.append("```")
    else:
        sections.append("_(none)_")
    sections.append("")
    sections.append("## Known limitations")
    sections.append("- None reported.")
    sections.append("")
    sections.append("## How to run")
    sections.append("1. Review the changed files above.")
    sections.append("2. Run your project test suite.")
    return "\n".join(sections)


def _compose_failure_summary(workflow_status: str, task_results: list[dict[str, Any]]) -> str:
    sections: list[str] = []
    sections.append(f"# Workflow ended: {workflow_status}")
    sections.append("")
    sections.append("## Why it didn't complete")
    failed = [r for r in task_results if r.get("status") not in ("DONE", None)]
    if failed:
        sections.append("Failed tasks:")
        for r in failed:
            sections.append(f"- {r.get('summary', r.get('status', '?'))}")
    else:
        sections.append("- No tasks completed before the workflow ended.")
    sections.append("")
    sections.append("## How to retry")
    sections.append("- Re-run the workflow; the previous JSONL log is preserved.")
    return "\n".join(sections)
