"""Planner agent — sub-role of the Chief.

Generates a planning document (Markdown) from the user's clarified
request. The Chief's planner role is responsible for the structure
defined in `prompts/planner.md`.

Phase 1: thin wrapper around the provider. Parses the Markdown
into a structured plan dict.
Phase 2.1: also runs `plan_parser.parse_plan` to populate the full
`ParsedPlan` AST. The legacy `_extract_task_table` regex is kept
for backwards compatibility with consumers that read `tasks`; it
will be removed in v0.3.
"""

from __future__ import annotations

import re
from typing import Any

from aco_runtime_lib.agents.base import Agent, AgentResult, AgentRole
from aco_runtime_lib.providers.base import ChatMessage, ChatRequest, ProviderError
from aco_runtime_lib.providers.router import ModelRouter
from aco_runtime_lib.workflow.plan_parser import (
    ParsedPlan,
    PlanParseError,
    parse_plan,
)

PLANNER_SYSTEM_PROMPT = """\
You are the Planner for Agent Company OS.

Produce a planning document in Markdown with this exact structure:

# Plan: <one-line title>
## Goal
## Architecture
## Task Graph (a Markdown table with columns: ID, Title, Owner Role, Depends On, Est. Tokens)
## APIs / Interfaces
## Data Model
## Acceptance Criteria (numbered list)
## Risks
## Out of Scope

Do not include any prose outside the plan document.
"""


class PlannerAgent(Agent):
    role = AgentRole.PLANNER

    def __init__(self, router: ModelRouter) -> None:
        self._router = router

    async def run(self, ctx: dict[str, Any]) -> AgentResult:
        provider, ref = self._router.pick("chief")
        request = ChatRequest(
            model=ref.model_id,
            messages=[
                ChatMessage(role="system", content=PLANNER_SYSTEM_PROMPT),
                ChatMessage(
                    role="user",
                    content=f"User request: {ctx.get('user_request', '')}",
                ),
            ],
            max_tokens=4096,
            temperature=0.4,
        )
        try:
            response = await provider.chat(request)
        except ProviderError as e:
            return AgentResult(
                role=self.role,
                data={"error": "provider_error", "message": str(e), "retryable": e.retryable},
            )
        plan_md = response.content.strip()
        tasks = _extract_task_table(plan_md)
        # Phase 2.1: full ParsedPlan AST. parse_plan may raise
        # PlanParseError on strict-mode failures (bad table,
        # unknown section, etc.) — surface it in the result so the
        # Chief's repair loop can feed the error back to the LLM
        # instead of crashing the workflow.
        try:
            parsed: ParsedPlan | None = parse_plan(plan_md)
            parse_error: dict[str, Any] | None = None
        except PlanParseError as e:
            parsed = None
            parse_error = {
                "section": e.section,
                "kind": e.kind,
                "line": e.line,
                "message": str(e),
            }
        return AgentResult(
            role=self.role,
            data={
                "plan_md": plan_md,
                "tasks": tasks,
                "parsed_plan": parsed,
                "parse_error": parse_error,
            },
        )


_TASK_ROW_RE = re.compile(
    r"^\|\s*(T\d+)\s*\|\s*([^|]+?)\s*\|\s*([^|]+?)\s*\|\s*([^|]+?)\s*\|\s*(\d[\d,]*)\s*\|",
    re.MULTILINE,
)


def _extract_task_table(plan_md: str) -> list[dict[str, Any]]:
    """Pull the Task Graph table out of a plan document."""
    out: list[dict[str, Any]] = []
    for m in _TASK_ROW_RE.finditer(plan_md):
        tokens_str = m.group(5).replace(",", "")
        out.append(
            {
                "id": m.group(1).strip(),
                "title": m.group(2).strip(),
                "owner_role": m.group(3).strip(),
                "depends_on": [
                    d.strip() for d in m.group(4).split(",") if d.strip() and d.strip() != "—"
                ],
                "est_tokens": int(tokens_str) if tokens_str.isdigit() else 0,
            }
        )
    return out
