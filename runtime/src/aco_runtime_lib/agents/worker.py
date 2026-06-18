"""Worker agent — executes one task and reports back.

Phase 1: in-process. The worker calls the provider with the task
envelope and parses a `TASK_RESULT` JSON. Real file edits happen
in Phase 1.5 via the Claude Code adapter (see
`crates/claude-adapter/`); for now the worker just returns what
the LLM says it would do.

See `prompts/worker.md` and `docs/AGENT_PROTOCOL.md` §5.1-§5.3.
"""

from __future__ import annotations

import json
import re
from typing import Any

from aco_runtime_lib.agents.base import Agent, AgentResult, AgentRole
from aco_runtime_lib.providers.base import (
    ChatMessage,
    ChatRequest,
    ProviderError,
)
from aco_runtime_lib.providers.router import ModelRouter

WORKER_SYSTEM_PROMPT = """\
You are a Worker in Agent Company OS. You will receive a single
task and must return a `TASK_RESULT` JSON object.

Output format (no prose, no markdown fences):

{
  "task_id": "<the id from the task envelope>",
  "status": "DONE" | "FAILED" | "PARTIAL",
  "summary": "<one-paragraph summary>",
  "files_modified": [
    {"path": "src/example.py", "lines_added": 5, "lines_removed": 2}
  ],
  "tests_run": {"passed": 0, "failed": 0, "skipped": 0}
}

If you are blocked, return:
{"error": "blocked", "reason": "..."}
"""


class WorkerAgent(Agent):
    """Generic worker. Spawned per-task by the Chief."""

    role = AgentRole.WORKER

    def __init__(self, router: ModelRouter) -> None:
        self._router = router

    async def run(self, ctx: dict[str, Any]) -> AgentResult:
        provider, ref = self._router.pick("worker")
        request = ChatRequest(
            model=ref.model_id,
            messages=[
                ChatMessage(role="system", content=WORKER_SYSTEM_PROMPT),
                ChatMessage(
                    role="user",
                    content=_render_task_envelope(ctx),
                ),
            ],
            max_tokens=2048,
            temperature=0.2,
        )
        try:
            response = await provider.chat(request)
        except ProviderError as e:
            return AgentResult(
                role=self.role,
                data={"error": "provider_error", "message": str(e), "retryable": e.retryable},
            )
        result = _parse_task_result(response.content)
        return AgentResult(role=self.role, data=result)


# ── Helpers ──────────────────────────────────────────────────────


def _render_task_envelope(ctx: dict[str, Any]) -> str:
    """Render a `TASK_ASSIGN` payload as the user-side prompt."""
    lines: list[str] = []
    lines.append(f"# Task: {ctx.get('title', '')}")
    lines.append(f"**Task ID:** {ctx.get('task_id', '')}")
    lines.append("")
    lines.append("## Objective")
    lines.append(ctx.get("objective", ""))
    lines.append("")
    interfaces = ctx.get("interfaces", {})
    if interfaces:
        lines.append("## Interfaces you consume")
        for c in interfaces.get("consumes", []):
            lines.append(f"- {c}")
        lines.append("")
        lines.append("## Interfaces you produce")
        for p in interfaces.get("produces", []):
            lines.append(f"- {p}")
        lines.append("")
    deps = ctx.get("dependencies", [])
    if deps:
        lines.append("## Dependencies (already done)")
        for d in deps:
            lines.append(f"- {d}")
        lines.append("")
    constraints = ctx.get("constraints", [])
    if constraints:
        lines.append("## Constraints")
        for c in constraints:
            lines.append(f"- {c}")
        lines.append("")
    deliverables = ctx.get("deliverables", [])
    if deliverables:
        lines.append("## Deliverables")
        for d in deliverables:
            lines.append(f"- {d}")
        lines.append("")
    budget = ctx.get("context_budget_tokens")
    if budget:
        lines.append(f"## Token budget\n{budget}")
    return "\n".join(lines)


_TASK_RESULT_RE = re.compile(r"\{.*\}", re.DOTALL)


def _parse_task_result(text: str) -> dict[str, Any]:
    matches = list(_TASK_RESULT_RE.finditer(text))
    for m in reversed(matches):
        try:
            parsed = json.loads(m.group(0))
        except json.JSONDecodeError:
            continue
        if isinstance(parsed, dict):
            return parsed
    return {"status": "FAILED", "summary": f"could not parse worker output: {text[:200]}"}
