"""ACO runtime — CLI entry point.

Run a complete workflow end-to-end with the deterministic
MockProvider (no API key required, no network).

Examples:
    python -m aco_runtime_lib demo is_prime
    python -m aco_runtime_lib demo "implement a hello function"
    python -m aco_runtime_lib --help

The mock provider scripts canned responses for every agent role
(Chief / Planner / Critic / Worker / Reporter / FinalReviewer),
so the whole orchestrator runs offline. Useful for:
* Newcomer onboarding ("see the workflow without setting up keys")
* Smoke test before a release
* Reproducing a deterministic run for a bug report
"""
from __future__ import annotations

import argparse
import asyncio
import sys
import time
from typing import Any

# Make the library importable both as a package and as a top-level
# script (when run with `python __main__.py`).
if __package__ in (None, ""):
    import os
    sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    __package__ = "aco_runtime_lib"

from aco_runtime_lib.event_bus import EventBus
from aco_runtime_lib.providers.mock import MockProvider
from aco_runtime_lib.providers.router import ModelRouter, RouterConfig
from aco_runtime_lib.workflow import OrchestratorOptions, WorkflowOrchestrator


# ── Scripted responses per agent role ──────────────────────────


CHIEF_CLEAR = (
    '{"status": "CLEAR", "reason": "the request is well-defined"}'
)

PLANNER_IS_PRIME = (
    "# Plan: is_prime(n)\n"
    "## Goal\n"
    "Write an `is_prime(n)` function and a pytest test.\n"
    "## Task Graph\n"
    "| ID | Title | Owner Role | Depends On | Est. Tokens |\n"
    "|----|-------|------------|------------|-------------|\n"
    "| T1 | Implement `is_prime` | Backend | — | 400 |\n"
    "| T2 | Write pytest | QA | T1 | 300 |\n"
    "## Acceptance Criteria\n"
    "1. `is_prime(7)` returns True\n"
    "2. `is_prime(1)` returns False\n"
    "## Risks\n"
    "- **Edge cases**: 0 and 1. Mitigated by explicit early return.\n"
    "## Out of Scope\n"
    "- Miller-Rabin primality test\n"
)

PLANNER_GENERIC = (
    "# Plan: {goal}\n"
    "## Goal\n"
    "{goal}\n"
    "## Task Graph\n"
    "| ID | Title | Owner Role | Depends On | Est. Tokens |\n"
    "|----|-------|------------|------------|-------------|\n"
    "| T1 | Implement the change | Backend | — | 400 |\n"
    "| T2 | Write tests | QA | T1 | 300 |\n"
    "## Acceptance Criteria\n"
    "1. The change runs as requested.\n"
    "## Risks\n"
    "- **Correctness**: edge cases. Mitigated by tests.\n"
    "## Out of Scope\n"
    "- Performance optimization\n"
)

CRITIC_PASS = (
    '{"verdict": "PASS", "confidence": 0.9, "issues": [], '
    '"summary": "the plan is sound"}'
)

WORKER_IS_PRIME = (
    '{"status": "DONE", '
    '"summary": "is_prime checks n>1 then trial-divides up to sqrt(n)", '
    '"files_modified": [{"path": "is_prime.py", "lines_added": 4, '
    '"lines_removed": 0}, {"path": "tests/test_is_prime.py", '
    '"lines_added": 5, "lines_removed": 0}]}'
)

WORKER_GENERIC = (
    '{"status": "DONE", '
    '"summary": "implemented the requested change with passing tests", '
    '"files_modified": [{"path": "main.py", "lines_added": 8, '
    '"lines_removed": 0}]}'
)

REPORTER_SUMMARY = (
    "# Delivery Summary\n\n"
    "## What was built\n"
    "- `is_prime(n)` function with O(sqrt(n)) trial-division.\n"
    "- pytest suite covering primes, composites, 0, 1, and 2.\n\n"
    "## Files modified\n"
    "- `is_prime.py` (4 lines)\n"
    "- `tests/test_is_prime.py` (5 lines)\n\n"
    "## How to run\n"
    "1. `pip install pytest`\n"
    "2. `pytest tests/test_is_prime.py`\n"
)

REPORTER_GENERIC = (
    "# Delivery Summary\n\n"
    "## What was built\n"
    "- The requested change with passing tests.\n\n"
    "## Files modified\n"
    "- `main.py` (8 lines)\n\n"
    "## How to run\n"
    "1. `python main.py`\n"
)

# ── Demo runner ──────────────────────────────────────────────────


def _make_router_and_orchestrator(user_request: str) -> tuple[ModelRouter, WorkflowOrchestrator, MockProvider]:
    """Build a router + orchestrator wired to a scripted MockProvider.

    The provider returns canned responses keyed on agent role.
    """
    mock = MockProvider()
    mock.when("You are the Chief", CHIEF_CLEAR)
    if "prime" in user_request.lower():
        mock.when("You are the Planner", PLANNER_IS_PRIME)
        mock.when("You are a Worker", WORKER_IS_PRIME)
        mock.when("composition", REPORTER_SUMMARY)
        mock.set_default(REPORTER_SUMMARY)
    else:
        plan = PLANNER_GENERIC.format(goal=user_request)
        mock.when("You are the Planner", plan)
        mock.when("You are a Worker", WORKER_GENERIC)
        mock.when("composition", REPORTER_GENERIC)
        mock.set_default(REPORTER_GENERIC)
    mock.when("You are a code reviewer", CRITIC_PASS)
    mock.when("Final Reviewer", CRITIC_PASS)

    router = ModelRouter(
        providers={"mock": mock},
        config=RouterConfig.from_toml_dict(
            defaults={"chief": "mock:mock", "critic_a": "mock:mock", "critic_b": "mock:mock",
                      "worker": "mock:mock", "reporter": "mock:mock"},
            fallbacks={},
        ),
    )
    bus = EventBus()
    options = OrchestratorOptions(enable_review=False, max_repair_loops=0)
    orchestrator = WorkflowOrchestrator(bus=bus, router=router, options=options)
    return router, orchestrator, mock


def _print_banner(user_request: str) -> None:
    print()
    print("=" * 70)
    print("ACO runtime demo — end-to-end multi-agent workflow")
    print("=" * 70)
    print(f"Request: {user_request!r}")
    print()


async def _run_demo(user_request: str) -> int:
    _print_banner(user_request)
    _, orchestrator, mock = _make_router_and_orchestrator(user_request)
    start = time.time()
    result = await orchestrator.run(
        wf_id=f"demo_{int(start)}",
        user_request=user_request,
    )
    elapsed = time.time() - start
    print()
    print("=" * 70)
    print(f"Result  state: {result.final_state.value}")
    print(f"        tasks : {len(result.task_results)}")
    print(f"        LLM   : {len(mock.calls)} calls")
    print(f"        time  : {elapsed:.2f}s")
    print("=" * 70)
    print()
    print("Delivery summary:")
    print("-" * 70)
    print(result.summary or "(no summary)")
    print("-" * 70)
    return 0 if result.final_state.value == "DONE" else 1


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="aco_runtime",
        description=(
            "ACO runtime. With no args, starts the FastAPI server on "
            ":7317. The 'demo' subcommand runs an end-to-end "
            "workflow with the deterministic MockProvider (no API key)."
        ),
    )
    sub = parser.add_subparsers(dest="cmd")
    demo_p = sub.add_parser("demo", help="run a demo workflow")
    demo_p.add_argument(
        "request",
        nargs="?",
        default="Write an is_prime function with pytest tests",
        help="user request for the multi-agent workflow",
    )
    args = parser.parse_args(argv)
    if args.cmd == "demo":
        return asyncio.run(_run_demo(args.request))
    if args.cmd is None:
        # Default: start the FastAPI server. This matches the
        # previous `aco-runtime` console script so the bundled
        # sidecar just works when launched from the Tauri shell.
        from aco_runtime.main import main as _server_main
        _server_main()
        return 0
    parser.print_help()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())