# Dogfooding: adding the `inspect` CLI

**Date:** 2026-06-20
**Workflow:** `wf_5b3c423859` (visible in `.validation/dogfooding/wf_5b3c423859/`)
**Request:** "Add a `python -m aco_runtime_lib inspect <wf_id>` CLI
subcommand that prints workflow history (events, plan, task
statuses, summary) from the persisted JSONL log. Support
`--summary`, `--events`, `--plan` flags. Add pytest tests."

## What ran end-to-end

ACO itself produced a 6-task plan, dispatched it through
Worker ×6 + Critic + Reporter + FinalReviewer. The actual model
calls were against the real `minimax-m3` API.

### Plan the model produced

| ID | Title | Owner | Est. Tokens | Final |
|----|-------|-------|-------------|-------|
| T1 | Locate existing CLI entry point, argparse pattern, JSONL log path | Backend | 400 | DONE |
| T2 | Implement `inspector.py` with `load_log`, `build_report` | Backend | 1500 | DONE |
| T3 | Add `inspect` subparser to `__main__.py` and wire the dispatch | Backend | 800 | RUNNING |
| T4 | Implement section formatters (summary, events, plan) | Backend | 1200 | DONE |
| T5 | Write pytest tests for inspector module and CLI | QA | 1000 | DONE |
| T6 | Document the new subcommand in README and CLI reference | Docs | 400 | DONE |

### What broke

The workflow finished **FAILED**. Final summary:

> `orchestrator error: no transition matches state=REPAIRING event='final_review_reject'`

### Root cause

`State.REPAIRING` had no transition for `final_review_reject` (or
`final_review_pass` / `final_review_repair`). The orchestrator
fires the same three events from both `FINAL_REVIEW` (after the
first Reporter run) and from `REPAIRING` (after a repair cycle
returns to the FinalReviewer). The state machine only knew the
first pair.

### Why this didn't show in unit tests

`tests/test_state_machine.py` walks the canonical happy path:
`REVIEWING → verdict_repair → REPAIRING → all_repaired →
REVIEWING → verdict_pass → DELIVERING → report_emitted →
FINAL_REVIEW → final_review_pass → DONE`. The FinalReviewer never
rejected in any test, so the REPAIRING → FAILED transition was
never exercised.

### Fix

`runtime/src/aco_runtime_lib/workflow/state_machine.py` — added
three transitions from `REPAIRING`:

```python
Transition(State.REPAIRING, "final_review_pass",   State.DONE),
Transition(State.REPAIRING, "final_review_repair", State.REPAIRING, _under_repair_budget),
Transition(State.REPAIRING, "final_review_reject", State.FAILED),
```

`runtime/tests/test_state_machine.py` — added
`test_final_review_reject_from_repairing` and
`test_final_review_pass_from_repairing` to prevent regression.
155 tests pass (was 153, +2 new).

## What this exposed about Phase 2 readiness

| Phase 2 capability | Status before | Status after fix |
|---|---|---|
| Real LLM planner produces useful DAGs | ✅ working (6 tasks, sensible titles) | ✅ |
| Worker emits parseable TASK_RESULT JSON | ⚠️ mixed — `python` and `git` plugins emit `DONE` cleanly, but the worker agent JSON parse path is fragile when content has `<think>` blocks or escapes | unchanged |
| FinalReviewer as gatekeeper | ❌ crashed on REJECT verdict from REPAIRING | ✅ now 3-state (PASS / REPAIR / REJECT) handled in both states |
| Live task_statuses propagation | ✅ working (verified earlier — UI shows T1..T6 updating live) | ✅ |
| Crash recovery from JSONL | ⚠️ only basic `find_resumable()` exists; no transactional rollback. Restart mid-REPAIRING would lose the repair decision | unchanged |
| Agent context handoff | ⚠️ the Worker sees only `envelope = {task_id, title, objective, deliverables}`. It does **not** see the planner's `## Goal` or `## Risks` sections, so each task is run blind to overall context | unchanged |

## What I (the CEO) learned

1. **The mock demo proves the wiring works but proves nothing
   about real workflows.** The real LLM planner produced 6 tasks
   for a request where the mock gave 2 — proving the planner's
   judgment (not the executor's) drives task count.
2. **State-machine coverage gaps hide in the diagonal.** The bug
   was invisible to the canonical happy path. Dogfooding forces
   the diagonal: "what if FinalReviewer REJECTS after a repair
   cycle?" — a corner a 13-test suite didn't reach.
3. **Tests still don't cover real agent JSON.** All 155 tests
   pass with the MockProvider. The dogfooding trace shows the
   worker JSON is sometimes unparseable (the model wraps in
   `<think>...</think>` blocks). A new test should use a small
   recorded MiniMax response to verify the parse path.

## What's in `.validation/dogfooding/wf_5b3c423859/`

```
plan.json   — parsed plan + live task_statuses from the moment
              T6 was RUNNING (5/6 done)
final.json  — final state + summary (FAILED) + task_results
              ([] because the workflow failed before any task
              result was stored)
```

## Action items for Phase 2.4 / 2.5

- [ ] Add REPAIRING → {pass, repair, reject} transitions (done
      in this commit)
- [ ] Worker JSON-parse test with a recorded real-LLM response
- [ ] Pass planner's Goal/Architecture/Risks sections into the
      worker's envelope so each task has project context
- [ ] Fix the parser's robustness to `<think>...</think>` blocks
- [ ] Drop the failed `wf_5b3c423859` artifacts in 30 days