"""Smoke-test Critic A after the false-negative fix.

Runs the same buggy code snippet that triggered the v0.2.1
validate_minimax false negative, with the full PRODUCTION Critic A
prompt (not the shortened one in validate_minimax.py).

Expectation after fix:
  * max_tokens bumped to 2048, so the verdict JSON is not truncated.
  * Model finds ≥ 1 issue (SQL injection, div-by-zero, etc.).
  * Verdict is REPAIR (or REWRITE), not PASS.
"""
import asyncio
import json
import sys
from pathlib import Path

from aco_runtime_lib.agents import AgentRole, CriticAgent
from aco_runtime_lib.event_bus import EventBus
from aco_runtime_lib.providers import MiniMaxProvider
from aco_runtime_lib.providers.router import ModelRouter, RouterConfig

PROMPT_PATH = Path("prompts/critic_a.md")
OUT_DIR = Path(".validation/outputs")
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


async def main() -> int:
    prompt = PROMPT_PATH.read_text(encoding="utf-8")
    p = MiniMaxProvider()
    cfg = RouterConfig.from_dict({"critic_a": "minimax:minimax-m3"})
    r = ModelRouter(providers={p.id: p}, config=cfg)
    critic = CriticAgent(
        role=AgentRole.CRITIC_A,
        router=r,
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
    print(json.dumps(out, indent=2, ensure_ascii=False))
    (OUT_DIR / "critic_a_smoke.json").write_text(
        json.dumps(out, indent=2, ensure_ascii=False), encoding="utf-8"
    )
    # Pass criteria: not PASS, at least 1 issue, and SQLi mentioned.
    summary_blob = (res.data["summary"] or "").lower() + " ".join(
        str(i) for i in res.data["issues"]
    )
    sqli_caught = "sql" in summary_blob and "inject" in summary_blob
    divzero_caught = "zerodivisionerror" in summary_blob or "b == 0" in summary_blob or "b == 0" in summary_blob or "divisor" in summary_blob
    parseage_caught = "valueerror" in summary_blob or "int(" in summary_blob
    print()
    print(f"verdict != PASS:         {res.data['verdict'] != 'PASS'}")
    print(f"issues >= 1:             {len(res.data['issues']) >= 1}")
    print(f"SQL injection caught:    {sqli_caught}")
    print(f"div-by-zero caught:      {divzero_caught}")
    print(f"parse_age ValueError:    {parseage_caught}")
    ok = (
        res.data["verdict"] != "PASS"
        and len(res.data["issues"]) >= 1
        and sqli_caught
    )
    print(f"\noverall: {'PASS' if ok else 'FAIL'}")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))