"""FinalReviewer agent.

A separate critic that runs AFTER all task-level reviews AND
after the Reporter has drafted the final delivery summary. The
FinalReviewer has **veto power** — its verdict can:

* **PASS**    -> the workflow moves to DONE
* **REPAIR**   -> the workflow loops back to the repair phase;
                  affected tasks are re-dispatched. The Reporter
                  must regenerate the delivery summary.
* **REJECT**   -> the workflow goes to FAILED. Used when the
                  delivery summary is fundamentally wrong (e.g.
                  the user request was misunderstood).

Why a separate role
===================
Critic A/B judge individual worker outputs at the per-task
granularity. The FinalReviewer judges the *whole delivery*:

* Does the final summary actually answer the user request?
* Are all claimed files real (could be checked via plugins)?
* Is anything contradicting or missing?

It uses the same prompt surface as a Critic but with a wider
context: the user_request, the full task_results, and the
drafted summary.
"""
from __future__ import annotations

from collections.abc import Mapping
from typing import Any

from aco_runtime_lib.agents.base import Agent, AgentResult, AgentRole


FINAL_REVIEWER_SYSTEM = (
    "You are the Final Reviewer of Agent Company OS. "
    "You run after the workers and the Reporter have produced a "
    "final delivery summary. Your job is to judge the WHOLE "
    "delivery against the user's original request. "
    "Reply with a single JSON object: "
    "{\"verdict\": \"PASS\"|\"REPAIR\"|\"REJECT\", "
    "\"confidence\": 0..1, "
    "\"issues\": [{\"severity\": \"MAJOR\"|\"MINOR\", "
    "\"message\": \"<short>\"}], "
    "\"summary\": \"<one paragraph explaining the verdict>\"}. "
    "Verdict rules: PASS only if the delivery substantively "
    "answers the user request and has no MAJOR issues. "
    "REPAIR if at least one MAJOR issue exists that can be "
    "fixed by re-running affected tasks. "
    "REJECT only if the delivery is fundamentally wrong (the "
    "whole workflow was a misunderstanding)."
)


class FinalReviewerAgent(Agent):
    """Reviews the final delivery summary. Verdict drives the
    transition out of FINAL_REVIEW (PASS -> DONE, REPAIR -> loop,
    REJECT -> FAILED).
    """

    role = AgentRole.FINAL_REVIEWER

    def __init__(self, system_prompt: str = FINAL_REVIEWER_SYSTEM) -> None:
        self._system_prompt = system_prompt

    async def run(self, ctx: Mapping[str, Any]) -> AgentResult:
        # Pure-function stub: the orchestrator drives this agent
        # via the model router in production. We keep the
        # in-process implementation as a deterministic fallback
        # that mirrors the wire protocol so tests can exercise
        # the orchestrator's final-review branch without a
        # model round-trip.
        user_request = str(ctx.get("user_request", ""))
        summary = str(ctx.get("summary", ""))
        task_results = ctx.get("task_results") or []
        verdict, confidence, issues = _deterministic_review(
            user_request, summary, task_results
        )
        return AgentResult(
            role=self.role,
            data={
                "verdict": verdict,
                "confidence": confidence,
                "issues": issues,
                "summary": _summary_text(verdict, user_request, summary, issues),
                "raw": "<deterministic stub>",
            },
        )


def _deterministic_review(
    user_request: str,
    summary: str,
    task_results: list[Any],
) -> tuple[str, float, list[dict[str, Any]]]:
    """Heuristic reviewer used in tests / when no model is wired.

    Returns (verdict, confidence, issues).

    Rules of thumb:
      * Empty user request       -> REJECT
      * Empty summary            -> REPAIR with 'no summary' MAJOR
      * No task_results          -> REPAIR with 'no tasks' MAJOR
      * Tasks all FAILED         -> REJECT
      * Tasks have MAJOR leftover -> REPAIR
      * Otherwise                -> PASS
    """
    issues: list[dict[str, Any]] = []
    if not user_request.strip():
        return "REJECT", 1.0, [
            {"severity": "MAJOR", "message": "user_request is empty"}
        ]
    if not summary.strip():
        issues.append({"severity": "MAJOR", "message": "no delivery summary"})
    if not task_results:
        issues.append({"severity": "MAJOR", "message": "no task results"})
    else:
        failed = [t for t in task_results if t.get("status") == "FAILED"]
        if failed and len(failed) == len(task_results):
            return "REJECT", 0.9, [
                {
                    "severity": "MAJOR",
                    "message": (
                        f"all {len(failed)} tasks FAILED; "
                        f"workflow cannot deliver value"
                    ),
                }
            ]
        if failed:
            for t in failed:
                issues.append({
                    "severity": "MAJOR",
                    "message": (
                        f"task {t.get('task_id', '?')} FAILED: "
                        f"{t.get('summary', '?')}"
                    ),
                })

    if any(i["severity"] == "MAJOR" for i in issues):
        return "REPAIR", 0.8, issues
    return "PASS", 0.9, issues


def _summary_text(
    verdict: str,
    user_request: str,
    summary: str,
    issues: list[dict[str, Any]],
) -> str:
    if verdict == "PASS":
        return (
            f"Final review passed. Delivery matches the request "
            f"({user_request[:80]}...)."
        )
    n = len(issues)
    return (
        f"Final review verdict: {verdict} ({n} issue(s)). "
        + "; ".join(i["message"][:80] for i in issues[:3])
    )
