/**
 * Regression tests for issue #26 defect 3 (desktop watchdog race):
 * A's late poll reply must never write B's state, and the timeout
 * branch must not fire for a workflow the loop no longer owns.
 *
 * The decisions live in `app/workflowPolling.ts` precisely so these
 * timings can be driven synchronously, without React or Tauri.
 */
import { describe, expect, it } from "vitest";
import {
  classifyWorkflowStatus,
  decidePollOutcome,
  isSameWorkflow,
  runWorkflowWatchdog,
  shouldApplyTimeoutVerdict,
  shouldKeepPolling,
  toPolledWorkflow,
  type PollBinding,
  type PollSnapshot,
  type WatchdogAction,
} from "./workflowPolling.js";

const binding = (epoch = 1, wfId = "wf-a"): PollBinding => ({ epoch, wfId });

const snapshot = (overrides: Partial<PollSnapshot> = {}): PollSnapshot => ({
  epoch: 1,
  currentWfId: "wf-a",
  busy: true,
  completed: false,
  ...overrides,
});

describe("classifyWorkflowStatus", () => {
  it("treats DONE / ABORTED* / FAILED* as terminal", () => {
    expect(classifyWorkflowStatus({ status: "DONE" }).terminal).toBe(true);
    expect(classifyWorkflowStatus({ status: "ABORTED: cancelled" }).terminal).toBe(true);
    expect(classifyWorkflowStatus({ status: "FAILED: boom" }).terminal).toBe(true);
    expect(classifyWorkflowStatus({ status: "RUNNING" }).terminal).toBe(false);
    expect(classifyWorkflowStatus({ status: "PENDING" }).terminal).toBe(false);
  });

  it("accepts the orchestrator delivery phase in either spelling", () => {
    expect(classifyWorkflowStatus({ phase: "delivery" }).delivery).toBe(true);
    expect(classifyWorkflowStatus({ phase: "8-delivery" }).delivery).toBe(true);
    expect(classifyWorkflowStatus({ phase: "develop" }).delivery).toBe(false);
  });

  it("falls back from status to state (matches the old invoke reply shape)", () => {
    expect(classifyWorkflowStatus({ state: "DONE" }).terminal).toBe(true);
  });

  it("reports a summary only when it is a non-empty string", () => {
    expect(classifyWorkflowStatus({ summary: "done!" }).summary).toBe("done!");
    expect(classifyWorkflowStatus({ summary: "" }).summary).toBeNull();
    expect(classifyWorkflowStatus(null).summary).toBeNull();
  });

  it("returns a neutral classification for missing replies", () => {
    expect(classifyWorkflowStatus(null)).toEqual({
      terminal: false,
      delivery: false,
      summary: null,
    });
    expect(classifyWorkflowStatus(undefined)).toEqual({
      terminal: false,
      delivery: false,
      summary: null,
    });
  });
});

describe("toPolledWorkflow", () => {
  it("normalizes the untyped invoke payload", () => {
    expect(
      toPolledWorkflow({ status: "DONE", phase: "delivery", summary: "ok", extra: 1 }),
    ).toEqual({ status: "DONE", phase: "delivery", summary: "ok" });
  });

  it("drops non-string fields instead of trusting them", () => {
    expect(toPolledWorkflow({ status: 42, phase: null, summary: {} })).toEqual({});
  });

  it("returns null for null / primitive payloads", () => {
    expect(toPolledWorkflow(null)).toBeNull();
    expect(toPolledWorkflow(undefined)).toBeNull();
    expect(toPolledWorkflow("nope")).toBeNull();
    expect(toPolledWorkflow(7)).toBeNull();
  });
});

describe("binding checks", () => {
  it("isSameWorkflow requires both epoch and wf id to match", () => {
    expect(isSameWorkflow(binding(1, "wf-a"), snapshot())).toBe(true);
    expect(isSameWorkflow(binding(1, "wf-a"), snapshot({ epoch: 2 }))).toBe(false);
    expect(isSameWorkflow(binding(1, "wf-a"), snapshot({ currentWfId: "wf-b" }))).toBe(false);
    expect(isSameWorkflow(binding(1, "wf-a"), snapshot({ currentWfId: null }))).toBe(false);
  });

  it("shouldKeepPolling requires the same workflow, busy and not completed", () => {
    expect(shouldKeepPolling(binding(), snapshot())).toBe(true);
    expect(shouldKeepPolling(binding(), snapshot({ busy: false }))).toBe(false);
    expect(shouldKeepPolling(binding(), snapshot({ completed: true }))).toBe(false);
    expect(shouldKeepPolling(binding(), snapshot({ epoch: 2 }))).toBe(false);
    expect(shouldKeepPolling(binding(), snapshot({ currentWfId: "wf-b" }))).toBe(false);
  });
});

describe("decidePollOutcome", () => {
  it("applies a terminal summary for the bound workflow", () => {
    expect(
      decidePollOutcome(binding(), snapshot(), { status: "DONE", summary: "全部完成" }),
    ).toEqual({ kind: "final-report", summary: "全部完成" });
  });

  it("applies a delivery-phase summary before the run is terminal", () => {
    expect(
      decidePollOutcome(binding(), snapshot(), {
        status: "RUNNING",
        phase: "delivery",
        summary: "交付",
      }),
    ).toEqual({ kind: "final-report", summary: "交付" });
  });

  it("marks terminal replies without a summary as complete only", () => {
    expect(decidePollOutcome(binding(), snapshot(), { status: "DONE" })).toEqual({
      kind: "mark-complete",
    });
    expect(decidePollOutcome(binding(), snapshot(), { status: "ABORTED" })).toEqual({
      kind: "mark-complete",
    });
  });

  it("keeps polling while the workflow is still running", () => {
    expect(
      decidePollOutcome(binding(), snapshot(), { status: "RUNNING", phase: "develop" }),
    ).toEqual({ kind: "continue" });
    expect(decidePollOutcome(binding(), snapshot(), null)).toEqual({ kind: "continue" });
  });

  it("ignores replies from a workflow the loop no longer owns", () => {
    // B started: the epoch bumped, but B's id has not reached the
    // reducer yet (SET_CURRENT_WF_ID fires after the start IPC).
    expect(
      decidePollOutcome(binding(1, "wf-a"), snapshot({ epoch: 2 }), {
        status: "DONE",
        summary: "A 的摘要",
      }),
    ).toEqual({ kind: "stale" });
    // ...and once B's id is visible.
    expect(
      decidePollOutcome(binding(1, "wf-a"), snapshot({ epoch: 2, currentWfId: "wf-b" }), {
        status: "DONE",
        summary: "A 的摘要",
      }),
    ).toEqual({ kind: "stale" });
  });

  it("ignores replies for a different wf id even inside the same epoch", () => {
    expect(
      decidePollOutcome(binding(1, "wf-a"), snapshot({ currentWfId: "wf-b" }), {
        status: "DONE",
        summary: "A 的摘要",
      }),
    ).toEqual({ kind: "stale" });
  });
});

describe("shouldApplyTimeoutVerdict", () => {
  it("fires for the bound workflow that is genuinely still busy", () => {
    expect(shouldApplyTimeoutVerdict(binding(), snapshot())).toBe(true);
  });

  it("does not fire once the bound workflow completed", () => {
    expect(shouldApplyTimeoutVerdict(binding(), snapshot({ completed: true }))).toBe(false);
    expect(shouldApplyTimeoutVerdict(binding(), snapshot({ busy: false }))).toBe(false);
  });
});

describe("regression: issue #26 defect 3 (A's stale loop vs. running B)", () => {
  it("does not mark B as timed out when A's loop exits because B took over", () => {
    const aLoop = binding(1, "wf-a");
    // User started B: epoch bumped synchronously; B is busy/running.
    const duringBStart = snapshot({ epoch: 2, currentWfId: "wf-a", busy: true, completed: false });
    // The loop exits (can no longer poll A)…
    expect(shouldKeepPolling(aLoop, duringBStart)).toBe(false);
    // …and must NOT enter the timeout branch and complete B.
    expect(shouldApplyTimeoutVerdict(aLoop, duringBStart)).toBe(false);

    const afterBId = snapshot({ epoch: 2, currentWfId: "wf-b", busy: true, completed: false });
    expect(shouldKeepPolling(aLoop, afterBId)).toBe(false);
    expect(shouldApplyTimeoutVerdict(aLoop, afterBId)).toBe(false);
  });

  it("never writes A's late reply into B", () => {
    const aLoop = binding(1, "wf-a");
    const bRunning = snapshot({ epoch: 2, currentWfId: "wf-b" });
    const outcome = decidePollOutcome(aLoop, bRunning, { status: "DONE", summary: "A 的摘要" });
    expect(outcome.kind).toBe("stale");
  });

  it("still honours the normal completion path for the bound workflow", () => {
    const aLoop = binding(1, "wf-a");
    const running = snapshot();
    expect(shouldKeepPolling(aLoop, running)).toBe(true);
    const outcome = decidePollOutcome(aLoop, running, { status: "DONE", summary: "A 完成" });
    expect(outcome).toEqual({ kind: "final-report", summary: "A 完成" });
    // Completed state means the timeout branch must stay quiet.
    expect(shouldApplyTimeoutVerdict(aLoop, snapshot({ busy: false, completed: true }))).toBe(
      false,
    );
  });
});
// ── Executable loop: `runWorkflowWatchdog` ─────────────────────
//
// The decision helpers above are only half the fix; these tests run
// the real watchdog loop with a fake clock + IPC so the A→B timing is
// exercised end to end (and would fail if the loop reverted to
// dispatching without re-checking the binding after the await).

interface Harness {
  state: PollSnapshot;
  dispatched: WatchdogAction[];
  fetchCalls: string[];
  run: () => Promise<void>;
}

function harness(opts: {
  initial: PollSnapshot;
  /** Defaults to initial.epoch / initial.currentWfId. */
  binding?: PollBinding;
  replies: Array<unknown | ((state: PollSnapshot) => unknown)>;
  timeoutMs?: number;
  timeoutSummary?: string;
  readWorkflowError?: () => string | null;
  onSleep?: (state: PollSnapshot, clock: number) => void;
  step?: number;
}): Harness {
  const state: PollSnapshot = { ...opts.initial };
  const dispatched: WatchdogAction[] = [];
  const fetchCalls: string[] = [];
  let clock = 0;
  let replyIdx = 0;
  const step = opts.step ?? 500;

  const fetchWorkflow = async (wfId: string): Promise<unknown> => {
    fetchCalls.push(wfId);
    const entry = opts.replies[Math.min(replyIdx, opts.replies.length - 1)];
    replyIdx += 1;
    return typeof entry === "function" ? (entry as (s: PollSnapshot) => unknown)(state) : entry;
  };

  return {
    state,
    dispatched,
    fetchCalls,
    run: () =>
      runWorkflowWatchdog({
        binding:
          opts.binding ??
          ({
            epoch: opts.initial.epoch,
            wfId: opts.initial.currentWfId ?? "wf-a",
          } satisfies PollBinding),
        readSnapshot: () => state,
        fetchWorkflow,
        dispatch: (action) => dispatched.push(action),
        sleep: async () => {
          clock += step;
          opts.onSleep?.(state, clock);
        },
        now: () => clock,
        timeoutMs: opts.timeoutMs ?? 10_000,
        timeoutSummary: opts.timeoutSummary ?? "超时",
        readWorkflowError: opts.readWorkflowError ?? (() => null),
      }),
  };
}

describe("runWorkflowWatchdog", () => {
  it("dispatches the normal completion exactly once and never the timeout verdict", async () => {
    const h = harness({
      initial: snapshot({ busy: true }),
      replies: [{ status: "DONE", summary: "A 完成" }],
    });
    await h.run();
    expect(h.fetchCalls).toEqual(["wf-a"]);
    expect(h.dispatched).toEqual([
      { type: "SET_FINAL_REPORT", report: "A 完成" },
      { type: "SET_BUSY", value: false },
      { type: "SET_COMPLETED", value: true },
    ]);
    // The fake reducer snapshot is intentionally still "busy" here:
    // React state lags the dispatch, and the old code would have
    // overwritten this completion with a timeout REPAIR verdict.
    expect(h.dispatched.some((a) => a.type === "SET_REVIEW_VERDICT")).toBe(false);
  });

  it("marks a genuinely timed-out bound workflow as REPAIR", async () => {
    const h = harness({
      initial: snapshot({ busy: true }),
      replies: [{ status: "RUNNING", phase: "develop" }],
      timeoutMs: 2_000,
      timeoutSummary: "看门狗超时",
    });
    await h.run();
    expect(h.dispatched).toEqual([
      {
        type: "SET_REVIEW_VERDICT",
        verdict: { verdict: "REPAIR", summary: "看门狗超时" },
      },
      { type: "SET_BUSY", value: false },
      { type: "SET_COMPLETED", value: true },
    ]);
  });

  it("prefers the latest workflow error over the timeout summary", async () => {
    const h = harness({
      initial: snapshot({ busy: true }),
      replies: [{ status: "RUNNING" }],
      timeoutMs: 2_000,
      readWorkflowError: () => "子进程崩溃",
    });
    await h.run();
    expect(h.dispatched[0]).toEqual({
      type: "SET_REVIEW_VERDICT",
      verdict: { verdict: "REPAIR", summary: "子进程崩溃" },
    });
  });

  it("writes nothing when B starts while A's poll IPC is in flight", async () => {
    const h = harness({
      initial: snapshot({ busy: true }),
      // A's reply resolves after B has already taken over: epoch and
      // wf id both changed while the IPC was pending.
      replies: [
        (state: PollSnapshot) => {
          state.epoch = 2;
          state.currentWfId = "wf-b";
          state.busy = true;
          state.completed = false;
          return { status: "DONE", summary: "A 的摘要" };
        },
      ],
    });
    await h.run();
    expect(h.fetchCalls).toEqual(["wf-a"]);
    expect(h.dispatched).toEqual([]);
  });

  it("writes nothing when B started before A's next poll", async () => {
    const h = harness({
      // The loop is still bound to A, but the live state is already B.
      initial: snapshot({ epoch: 2, currentWfId: "wf-b", busy: true }),
      binding: { epoch: 1, wfId: "wf-a" },
      replies: [{ status: "DONE", summary: "A 的摘要" }],
    });
    await h.run();
    expect(h.fetchCalls).toEqual([]);
    expect(h.dispatched).toEqual([]);
  });

  it("does not time out B when it replaces A exactly at the deadline", async () => {
    const h = harness({
      initial: snapshot({ busy: true }),
      replies: [{ status: "RUNNING" }],
      timeoutMs: 2_000,
      step: 400,
      onSleep: (state, clock) => {
        // B starts on the final sleep, exactly when A's budget expires.
        if (clock >= 2_000) {
          state.epoch = 2;
          state.currentWfId = "wf-b";
        }
      },
    });
    await h.run();
    expect(h.dispatched).toEqual([]);
  });

  it("stops when the event stream already completed the bound workflow", async () => {
    const h = harness({
      initial: snapshot({ busy: false, completed: true }),
      replies: [{ status: "RUNNING" }],
    });
    await h.run();
    expect(h.fetchCalls).toEqual([]);
    expect(h.dispatched).toEqual([]);
  });
});
