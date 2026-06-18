"""Validate ACO's agent layer against the real MiniMax M3 API.

Runs 2 end-to-end tasks:

1. **Planning task** — feed the Planner agent a real user request;
   verify it returns a structured plan with the expected sections.
2. **Code review task** — feed the Critic A agent a real code
   snippet; verify it returns a structured verdict.

Outputs land in `.validation/outputs/` (under the project root, not
on C:\\). The directory is removed by the cleanup script after a
successful run.

Run with: `python .validation/validate_minimax.py`
"""

from __future__ import annotations

import asyncio
import json
import os
import sys
from datetime import UTC, datetime
from pathlib import Path

from aco_runtime_lib.agents import (
    AgentRole,
    CriticAgent,
    PlannerAgent,
)
from aco_runtime_lib.event_bus import EventBus
from aco_runtime_lib.providers import (
    MiniMaxProvider,
    ProviderError,
)
from aco_runtime_lib.providers.router import (
    ModelRef,
    RouterConfig,
    ModelRouter,
)

HERE = Path(__file__).parent
# Outputs land in `.validation/outputs/` (gitignored) so the user
# can re-run and `rm -rf .validation` cleanly. See .gitignore.
OUT_DIR = HERE.parent / ".validation" / "outputs"
OUT_DIR.mkdir(parents=True, exist_ok=True)

MODEL = "minimax-m3"


def _now_iso() -> str:
    return datetime.now(UTC).isoformat(timespec="milliseconds").replace("+00:00", "Z")


def _build_router() -> ModelRouter:
    """Build a router with MiniMax M3 wired up for all roles."""
    try:
        provider = MiniMaxProvider()
    except ProviderError as e:
        print(f"FATAL: {e}", file=sys.stderr)
        print("Set MINIMAX_API_KEY before running.", file=sys.stderr)
        sys.exit(1)
    providers = {provider.id: provider}
    config = RouterConfig.from_dict(
        {
            "chief": f"{provider.id}:{MODEL}",
            "critic_a": f"{provider.id}:{MODEL}",
            "critic_b": f"{provider.id}:{MODEL}",
            "worker": f"{provider.id}:{MODEL}",
            "reporter": f"{provider.id}:{MODEL}",
        }
    )
    return ModelRouter(providers=providers, config=config)


# ── Tasks ────────────────────────────────────────────────────────


async def task_1_planning(router: ModelRouter) -> dict[str, object]:
    """Task 1: ask MiniMax to plan a real feature.

    Verifies the agent layer (PlannerAgent) end-to-end against a
    real LLM: prompt rendering → HTTP call → JSON parse → output.
    """
    bus = EventBus()
    planner = PlannerAgent(router=router)

    user_request = (
        "Add a `/users/<id>/avatar` endpoint that accepts a multipart "
        "image upload, validates it's a PNG or JPEG, resizes to 256x256, "
        "and stores it in the `user_avatars` S3 bucket. Returns a 200 "
        "with the avatar URL."
    )
    started = _now_iso()
    try:
        result = await planner.run({"user_request": user_request})
    except ProviderError as e:
        return {
            "task": "1-planning",
            "started_at": started,
            "ended_at": _now_iso(),
            "ok": False,
            "error": str(e),
            "retryable": e.retryable,
        }
    ended = _now_iso()

    plan_md = result.data.get("plan_md", "")
    tasks = result.data.get("tasks", [])
    sections_present = {
        "Goal": "## Goal" in plan_md,
        "Architecture": "## Architecture" in plan_md,
        "Task Graph": "## Task Graph" in plan_md,
        "APIs/Interfaces": "## APIs / Interfaces" in plan_md,
        "Data Model": "## Data Model" in plan_md,
        "Acceptance Criteria": "## Acceptance Criteria" in plan_md,
        "Risks": "## Risks" in plan_md,
        "Out of Scope": "## Out of Scope" in plan_md,
    }
    return {
        "task": "1-planning",
        "started_at": started,
        "ended_at": ended,
        "ok": True,
        "sections_present": sections_present,
        "all_sections": all(sections_present.values()),
        "task_count": len(tasks),
        "plan_chars": len(plan_md),
        "plan_preview": plan_md[:500],
    }


async def task_2_code_review(router: ModelRouter) -> dict[str, object]:
    """Task 2: ask MiniMax (Critic A) to review a small Python function.

    Verifies that Critic A returns a structured verdict JSON.
    """
    bus = EventBus()
    critic = CriticAgent(
        role=AgentRole.CRITIC_A,
        router=router,
        bus=bus,
        router_role="critic_a",
        system_prompt=(
            "You are Critic A, a focused bug-hunter. Review code for "
            "runtime errors, edge cases, and security issues. "
            "Always emit a JSON object with `verdict`, `confidence`, "
            "`issues`, and `summary`."
        ),
    )

    code_snippet = '''\
def divide_numbers(a, b):
    """Divide a by b."""
    return a / b

def read_user(user_id):
    """Read a user from the database."""
    sql = f"SELECT * FROM users WHERE id = {user_id}"
    return db.execute(sql)

def parse_age(s):
    """Parse an age string."""
    return int(s)
'''
    started = _now_iso()
    try:
        result = await critic.run(
            {
                "subject": "utils.py — three helper functions",
                "ask": (
                    "Look for runtime errors, edge cases, and security "
                    "holes in the following three functions:\n\n"
                    "```python\n" + code_snippet + "\n```\n\n"
                    "Specifically: divide_numbers divides two numbers, "
                    "read_user runs a SQL query built from user input, "
                    "parse_age parses an integer from a string."
                ),
                "files": ["utils.py"],
            }
        )
    except ProviderError as e:
        return {
            "task": "2-code-review",
            "started_at": started,
            "ended_at": _now_iso(),
            "ok": False,
            "error": str(e),
            "retryable": e.retryable,
        }
    ended = _now_iso()

    verdict = result.data.get("verdict", "MISSING")
    confidence = result.data.get("confidence", 0.0)
    issues = result.data.get("issues", [])
    summary = result.data.get("summary", "")
    return {
        "task": "2-code-review",
        "started_at": started,
        "ended_at": ended,
        "ok": True,
        "verdict": verdict,
        "verdict_is_valid": verdict in ("PASS", "REPAIR", "REWRITE"),
        "confidence": confidence,
        "issue_count": len(issues),
        "issues": issues,
        "summary": summary,
    }


# ── Main ─────────────────────────────────────────────────────────


async def main() -> int:
    print(f"Validating ACO with MiniMax M3 — {_now_iso()}")
    print(f"Output dir: {OUT_DIR}")
    print()

    router = _build_router()
    print(f"Router up. Providers: {router.available}")
    print(f"Defaults: chief={router._config.defaults['chief']}")  # noqa: SLF001
    print()

    summary = {
        "started_at": _now_iso(),
        "model": f"minimax:{MODEL}",
        "minimax_key_env": "MINIMAX_API_KEY",
        "minimax_key_present": bool(os.environ.get("MINIMAX_API_KEY")),
    }

    # Task 1
    print("─" * 60)
    print("Task 1: planning")
    print("─" * 60)
    r1 = await task_1_planning(router)
    print(f"  ok: {r1.get('ok')}")
    if r1.get("ok"):
        print(f"  sections present: {r1['sections_present']}")
        print(f"  all sections: {r1['all_sections']}")
        print(f"  tasks: {r1['task_count']}")
        print(f"  plan chars: {r1['plan_chars']}")
    else:
        print(f"  error: {r1.get('error')}")
    summary["task_1"] = r1
    (OUT_DIR / "task_1_planning.json").write_text(
        json.dumps(r1, indent=2, ensure_ascii=False), encoding="utf-8"
    )
    print()

    # Task 2
    print("─" * 60)
    print("Task 2: code review")
    print("─" * 60)
    r2 = await task_2_code_review(router)
    print(f"  ok: {r2.get('ok')}")
    if r2.get("ok"):
        print(f"  verdict: {r2['verdict']}")
        print(f"  valid verdict: {r2['verdict_is_valid']}")
        print(f"  confidence: {r2['confidence']}")
        print(f"  issues: {r2['issue_count']}")
        print(f"  summary: {r2['summary'][:200]}")
    else:
        print(f"  error: {r2.get('error')}")
    summary["task_2"] = r2
    (OUT_DIR / "task_2_code_review.json").write_text(
        json.dumps(r2, indent=2, ensure_ascii=False), encoding="utf-8"
    )
    print()

    summary["ended_at"] = _now_iso()
    summary["all_passed"] = bool(r1.get("ok")) and bool(r2.get("ok"))
    (OUT_DIR / "summary.json").write_text(
        json.dumps(summary, indent=2, ensure_ascii=False), encoding="utf-8"
    )

    print("─" * 60)
    print(f"All passed: {summary['all_passed']}")
    print("─" * 60)
    return 0 if summary["all_passed"] else 1


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
