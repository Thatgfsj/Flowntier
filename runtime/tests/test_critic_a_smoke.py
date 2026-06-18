"""Smoke-test Critic A against the real MiniMax M3 API.

Runs the same buggy code snippet that triggered the v0.2.1
validate_minimax false negative, with the full PRODUCTION Critic A
prompt (not the shortened one in scripts/validate_minimax.py).

Expectation after the 2026-06-18 fix:
  * max_tokens bumped to 2048, so the verdict JSON is not truncated.
  * Model finds ≥ 1 issue (SQL injection, div-by-zero, etc.).
  * Verdict is REPAIR (or REWRITE), not PASS.

This test is a real-API integration test. It is **skipped** unless
``MINIMAX_API_KEY`` is set, so it doesn't break local dev / CI that
doesn't have a key.

Run explicitly::

    MINIMAX_API_KEY=sk-... uv run pytest tests/test_critic_a_smoke.py -v -s
"""
from __future__ import annotations

import asyncio
import json
import os
from pathlib import Path

import pytest

pytestmark = pytest.mark.skipif(
    not os.environ.get("MINIMAX_API_KEY"),
    reason="MINIMAX_API_KEY not set; integration test against real API",
)

PROMPT_PATH = (
    Path(__file__).resolve().parent.parent.parent / "prompts" / "critic_a.md"
)
OUT_DIR = (
    Path(__file__).resolve().parent.parent.parent
    / ".validation"
    / "outputs"
)
OUT_DIR.mkdir(parents=True, exist_ok=True)

CODE = '''\
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


@pytest.mark.asyncio
async def test_critic_a_catches_sqli_in_obvious_buggy_snippet() -> None:
    from aco_runtime_lib.agents import AgentRole, CriticAgent
    from aco_runtime_lib.event_bus import EventBus
    from aco_runtime_lib.providers import MiniMaxProvider
    from aco_runtime_lib.providers.router import ModelRouter, RouterConfig

    prompt = PROMPT_PATH.read_text(encoding="utf-8")
    provider = MiniMaxProvider()
    cfg = RouterConfig.from_dict({"critic_a": "minimax:minimax-m3"})
    router = ModelRouter(providers={provider.id: provider}, config=cfg)
    critic = CriticAgent(
        role=AgentRole.CRITIC_A,
        router=router,
        bus=EventBus(),
        router_role="critic_a",
        system_prompt=prompt,
    )
    res = await critic.run(
        {
            "subject": "utils.py — three helper functions",
            "ask": (
                "Look for runtime errors, edge cases, and security "
                "holes:\n\n```python\n" + CODE + "\n```\n\nSpecifically: "
                "divide_numbers divides two numbers, read_user runs a SQL "
                "query built from user input, parse_age parses an integer "
                "from a string."
            ),
            "files": ["utils.py"],
        }
    )

    out = {
        "verdict": res.data["verdict"],
        "confidence": res.data["confidence"],
        "issue_count": len(res.data["issues"]),
        "issues": res.data["issues"],
        "summary_first_500": res.data["summary"][:500],
        "summary_total_chars": len(res.data["summary"]),
    }
    (OUT_DIR / "critic_a_smoke.json").write_text(
        json.dumps(out, indent=2, ensure_ascii=False), encoding="utf-8"
    )

    # Pass criteria
    summary_blob = (res.data["summary"] or "").lower() + " ".join(
        str(i) for i in res.data["issues"]
    ).lower()
    sqli_caught = "sql" in summary_blob and "inject" in summary_blob
    divzero_caught = (
        "zerodivisionerror" in summary_blob or "divisor" in summary_blob
    )
    parseage_caught = "valueerror" in summary_blob or "int(" in summary_blob

    assert res.data["verdict"] != "PASS", (
        f"verdict must not be PASS on a snippet with obvious bugs; "
        f"got {res.data['verdict']!r}, summary={res.data['summary'][:300]!r}"
    )
    assert len(res.data["issues"]) >= 1, (
        "must find at least 1 issue (SQLi, div-by-zero, or ValueError)"
    )
    assert sqli_caught, (
        "must catch the SQL injection in read_user — this was the "
        "v0.2.1 validate_minimax false negative"
    )
    # Bonus checks (logged but not asserted, since model phrasing varies)
    print(f"\nverdict={res.data['verdict']} issues={len(res.data['issues'])}")
    print(f"  div-by-zero caught: {divzero_caught}")
    print(f"  parse_age caught:   {parseage_caught}")