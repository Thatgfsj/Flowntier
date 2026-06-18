"""ACO end-to-end smoke test.

Submits a workflow via POST /api/workflow, polls /plan + /api/workflow
until terminal state, prints a compact result table. Designed to be
invoked multiple times in a session and self-clean outputs at the end.

Usage::

    python .validation/e2e_smoke.py "implement a /login API"
    python .validation/e2e_smoke.py "实现用户登录接口" --label cn
    python .validation/e2e_smoke.py "" --label empty --expect-error

Exit code is 0 if the workflow reached a terminal state (DONE or
FAILED) and no parse_error / validation_error surfaced, unless
--expect-error is set (then exit 0 means the expected error fired).
"""
from __future__ import annotations

import argparse
import json
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

RUNTIME = "http://127.0.0.1:7317"
OUT_DIR = Path(".validation/outputs")
OUT_DIR.mkdir(parents=True, exist_ok=True)


def post(path: str, body: dict | None = None) -> dict:
    data = json.dumps(body or {}).encode("utf-8")
    req = urllib.request.Request(
        f"{RUNTIME}{path}",
        data=data,
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=30) as r:
        return json.loads(r.read())


def get(path: str) -> dict:
    # /plan and /workflow can be slow when the orchestrator is busy
    # (planning phase alone takes ~1 min for big requests).
    with urllib.request.urlopen(f"{RUNTIME}{path}", timeout=60) as r:
        return json.loads(r.read())


def poll_workflow(wf_id: str, timeout: float = 300.0) -> dict:
    """Poll /api/workflow/{id} every 2s until terminal state."""
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            d = get(f"/api/workflow/{wf_id}")
            if d.get("state") in ("DONE", "FAILED", "ABORTED"):
                return d
        except urllib.error.URLError:
            pass
        time.sleep(2)
    raise TimeoutError(f"workflow {wf_id} did not finish in {timeout}s")


def main() -> int:
    p = argparse.ArgumentParser()
    p.add_argument("text", help="workflow request text")
    p.add_argument("--label", default="run", help="output file label")
    p.add_argument("--expect-error", action="store_true",
                   help="expect parse/validation error (exit 0 if it fires)")
    p.add_argument("--timeout", type=float, default=300.0)
    args = p.parse_args()

    started = time.time()
    # Force UTF-8 stdout so emoji / non-ASCII text doesn't crash on
    # Windows consoles that default to GBK.
    try:
        sys.stdout.reconfigure(encoding="utf-8")
    except Exception:
        pass
    print(f"[e2e] label={args.label!r} text={args.text!r}")

    # 1. Submit
    r = post("/api/workflow", {"text": args.text})
    wf_id = r["id"]
    print(f"[e2e] submitted wf_id={wf_id}")

    # 2. Poll /plan (just for the snapshot — final state captured later)
    try:
        plan_snap = get(f"/api/workflow/{wf_id}/plan")
    except Exception as e:
        plan_snap = {"error": str(e)}
    parsed = plan_snap.get("parsed_plan")
    nodes = (parsed or {}).get("nodes") if parsed else []
    parse_err = plan_snap.get("parse_error")
    val_err = plan_snap.get("validation_error")
    task_statuses = plan_snap.get("task_statuses") or {}
    print(f"[e2e] plan snapshot: nodes={len(nodes)} "
          f"task_statuses={len(task_statuses)} "
          f"parse_error={bool(parse_err)} "
          f"validation_error={bool(val_err)}")

    # 3. Poll workflow completion
    final = poll_workflow(wf_id, args.timeout)
    elapsed = time.time() - started
    summary = final.get("summary") or ""
    print(f"[e2e] final state={final.get('state')!r} "
          f"elapsed={elapsed:.1f}s summary_chars={len(summary)}")

    # 4. Save outputs
    out = {
        "label": args.label,
        "wf_id": wf_id,
        "elapsed_sec": round(elapsed, 1),
        "plan_snap": plan_snap,
        "final_state": final.get("state"),
        "summary_preview": summary[:500],
    }
    out_path = OUT_DIR / f"e2e_{args.label}.json"
    out_path.write_text(json.dumps(out, indent=2, ensure_ascii=False),
                       encoding="utf-8")
    print(f"[e2e] wrote {out_path}")

    # 5. Verdict
    ok = True
    if final.get("state") not in ("DONE", "FAILED", "ABORTED"):
        print(f"[e2e] FAIL: never reached terminal state")
        ok = False
    if not args.expect_error and (parse_err or val_err):
        print(f"[e2e] FAIL: parse_error or validation_error fired")
        ok = False
    if args.expect_error and not (parse_err or val_err):
        print(f"[e2e] FAIL: --expect-error set but no error fired")
        ok = False
    if final.get("state") == "DONE" and len(task_statuses) != len(nodes):
        print(f"[e2e] FAIL: task_statuses count ({len(task_statuses)}) "
              f"!= nodes count ({len(nodes)})")
        ok = False

    print(f"[e2e] verdict: {'PASS' if ok else 'FAIL'}")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())