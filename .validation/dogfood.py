"""Dogfooding harness — submit a real workflow task to ACO, poll
until done, dump everything (plan, events, task statuses,
summary, runtime log slice) into a per-wf folder.

Usage:
    python .validation/dogfood.py "<user_request>" [--timeout 600]

Outputs go to .validation/dogfooding/<wf_id>/.
"""
from __future__ import annotations

import argparse
import json
import os
import sys
import time
import urllib.request

RUNTIME = "http://127.0.0.1:7317"


def http_post(path: str, body: dict, timeout: int = 30) -> dict:
    req = urllib.request.Request(
        f"{RUNTIME}{path}",
        data=json.dumps(body).encode("utf-8"),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read())


def http_get(path: str, timeout: int = 30) -> dict | list:
    req = urllib.request.Request(f"{RUNTIME}{path}")
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read())


def main() -> int:
    p = argparse.ArgumentParser()
    p.add_argument("request", help="user request for ACO")
    p.add_argument("--timeout", type=int, default=600)
    p.add_argument("--out", default=".validation/dogfooding")
    args = p.parse_args()

    print(f"=== submitting ===")
    print(f"  request: {args.request!r}")
    submit = http_post("/api/workflow", {"text": args.request})
    wf_id = submit["id"]
    print(f"  wf_id  : {wf_id}")

    out_dir = os.path.join(args.out, wf_id)
    os.makedirs(out_dir, exist_ok=True)

    print(f"\n=== polling (timeout={args.timeout}s) ===")
    start = time.time()
    deadline = start + args.timeout
    final = None
    last_state = None
    while time.time() < deadline:
        time.sleep(10)
        try:
            r = http_get(f"/api/workflow/{wf_id}", timeout=10)
        except Exception as e:
            print(f"  poll error: {e}")
            continue
        last_state = r.get("state")
        elapsed = time.time() - start
        print(f"  t={elapsed:6.1f}s  state={last_state}  "
              f"tasks={len(r.get('task_results') or [])}  "
              f"summary_chars={len(r.get('summary') or '')}")
        if last_state in ("DONE", "FAILED", "ABORTED"):
            final = r
            break
        # Save plan + tasks snapshot every poll (they update live)
        try:
            plan = http_get(f"/api/workflow/{wf_id}/plan", timeout=10)
            with open(os.path.join(out_dir, "plan.json"), "w") as f:
                json.dump(plan, f, indent=2, ensure_ascii=False)
        except Exception:
            pass

    if final is None:
        print(f"!! timeout, last state = {last_state}")
        return 1

    # Save final artifacts
    with open(os.path.join(out_dir, "final.json"), "w") as f:
        json.dump(final, f, indent=2, ensure_ascii=False)

    print(f"\n=== final state: {final['state']} ===")
    print(f"  summary ({len(final.get('summary') or '')} chars):")
    print(f"  {'-' * 60}")
    print(f"  {(final.get('summary') or '(none)')[:600]}")
    print(f"  {'-' * 60}")

    # Pretty task list
    print(f"\n  task results ({len(final.get('task_results') or [])}):")
    for t in final.get("task_results") or []:
        print(f"    {t.get('task_id', '?')}: {t.get('status', '?')} "
              f"— {t.get('title', '?')[:50]}")

    print(f"\n=== artifacts in {out_dir} ===")
    print(f"  plan.json  : parsed plan + live task statuses")
    print(f"  final.json : workflow state + summary + task_results")
    return 0 if final["state"] == "DONE" else 2


if __name__ == "__main__":
    sys.exit(main())