/**
 * Pure decision helpers for the desktop workflow watchdog
 * (App.tsx → `startRealWorkflow`). Extracted so the timing races
 * tracked in issue #26 (defect 3) are unit-testable without React
 * or Tauri.
 *
 * The watchdog polls `get_workflow(id)` every 2s and used to
 * dispatch the reply into the reducer after checking only that the
 * *pre-call* state still matched. Two races followed:
 *
 *   1. A's in-flight reply could land after the user started B and
 *      write A's summary / "completed" flags into B's UI state.
 *   2. When the loop noticed the workflow id had changed it exited,
 *      but the timeout branch below still ran — marking the
 *      still-running B as timed out and completed.
 *
 * A `PollBinding` is the `(epoch, wfId)` pair a loop is bound to.
 * `epoch` is bumped synchronously when a new run starts, so a stale
 * reply is rejected even during the window where B is starting but
 * `SET_CURRENT_WF_ID` has not reached the reducer yet.
 */

export interface PollBinding {
  /** Monotonic generation counter; one per workflow start. */
  epoch: number;
  /** Workflow id this loop watches. */
  wfId: string;
}

/** The reducer fields the watchdog bases its decisions on. */
export interface PollSnapshot {
  /** Current generation (App.tsx `wfEpochRef.current`). */
  epoch: number;
  /** Current workflow id in the reducer. */
  currentWfId: string | null;
  busy: boolean;
  completed: boolean;
}

/** The subset of the `get_workflow` reply the watchdog uses. */
export interface PolledWorkflow {
  status?: string;
  state?: string;
  phase?: string;
  summary?: string;
}

export interface WorkflowStatusClass {
  /** DONE / ABORTED* / FAILED* — the run will not progress any more. */
  terminal: boolean;
  /** Orchestrator's final delivery phase. */
  delivery: boolean;
  /** Summary, when the sidecar supplied a non-empty one. */
  summary: string | null;
}

/** Classify a normalized workflow reply. */
export function classifyWorkflowStatus(wf: PolledWorkflow | null | undefined): WorkflowStatusClass {
  if (!wf) return { terminal: false, delivery: false, summary: null };
  const status = wf.status || wf.state || null;
  const terminal =
    status === "DONE" ||
    (status !== null && (status.startsWith("ABORTED") || status.startsWith("FAILED")));
  const phase = wf.phase ?? null;
  const delivery = phase === "delivery" || phase === "8-delivery";
  const summary = typeof wf.summary === "string" && wf.summary.length > 0 ? wf.summary : null;
  return { terminal, delivery, summary };
}

/**
 * Normalize the untyped `invoke("get_workflow")` reply. Returns null
 * for null/non-object payloads so callers can treat "nothing usable"
 * as "keep polling" instead of crashing.
 */
export function toPolledWorkflow(raw: unknown): PolledWorkflow | null {
  if (raw === null || typeof raw !== "object") return null;
  const r = raw as Record<string, unknown>;
  const out: PolledWorkflow = {};
  if (typeof r.status === "string") out.status = r.status;
  if (typeof r.state === "string") out.state = r.state;
  if (typeof r.phase === "string") out.phase = r.phase;
  if (typeof r.summary === "string") out.summary = r.summary;
  return out;
}

/** Is `snapshot` still describing the workflow this loop is bound to? */
export function isSameWorkflow(binding: PollBinding, snapshot: PollSnapshot): boolean {
  return snapshot.epoch === binding.epoch && snapshot.currentWfId === binding.wfId;
}

/** Keep polling only while the bound workflow is still running. */
export function shouldKeepPolling(binding: PollBinding, snapshot: PollSnapshot): boolean {
  return isSameWorkflow(binding, snapshot) && snapshot.busy && !snapshot.completed;
}

/**
 * The timeout branch may only fire for the bound workflow and only
 * while it is still busy. A workflow that has been replaced must
 * never be written as timed out.
 */
export function shouldApplyTimeoutVerdict(binding: PollBinding, snapshot: PollSnapshot): boolean {
  return isSameWorkflow(binding, snapshot) && snapshot.busy && !snapshot.completed;
}

export type PollOutcome =
  /** The reply belongs to a workflow this loop no longer owns. */
  | { kind: "stale" }
  /** Nothing actionable yet (or an unusable reply); keep polling. */
  | { kind: "continue" }
  /** Terminal/delivery reply with a summary → report + complete. */
  | { kind: "final-report"; summary: string }
  /** Terminal reply without a summary → just complete. */
  | { kind: "mark-complete" };

/**
 * Decide what a `get_workflow` reply should do, re-checking the
 * binding *after* the IPC await (the caller reads a fresh snapshot).
 */
export function decidePollOutcome(
  binding: PollBinding,
  snapshot: PollSnapshot,
  raw: unknown,
): PollOutcome {
  if (!isSameWorkflow(binding, snapshot)) return { kind: "stale" };
  const wf = toPolledWorkflow(raw);
  if (wf === null) return { kind: "continue" };
  const { terminal, delivery, summary } = classifyWorkflowStatus(wf);
  if (summary !== null && (terminal || delivery)) {
    return { kind: "final-report", summary };
  }
  if (terminal) return { kind: "mark-complete" };
  return { kind: "continue" };
}

/** Reducer actions the watchdog is allowed to emit. */
export type WatchdogAction =
  | { type: "SET_FINAL_REPORT"; report: string }
  | { type: "SET_BUSY"; value: boolean }
  | { type: "SET_COMPLETED"; value: boolean }
  | { type: "SET_REVIEW_VERDICT"; verdict: { verdict: "REPAIR"; summary: string } };

export interface WatchdogOptions {
  /** Workflow this watchdog invocation owns. */
  binding: PollBinding;
  /** Read the live reducer snapshot (called before and after IPC). */
  readSnapshot: () => PollSnapshot;
  /** `get_workflow` IPC; injected so tests can control timing. */
  fetchWorkflow: (wfId: string) => Promise<unknown>;
  /** Reducer dispatch. */
  dispatch: (action: WatchdogAction) => void;
  /** Sleep between polls; injected for tests. */
  sleep: (ms: number) => Promise<void>;
  /** Wall clock; injected for tests. */
  now: () => number;
  /** Total watchdog budget (App uses 30 minutes). */
  timeoutMs: number;
  /** Verdict summary used when the bound workflow runs out of budget. */
  timeoutSummary: string;
  /** Latest workflow error, if any (overrides `timeoutSummary`). */
  readWorkflowError: () => string | null;
  /** Interval between polls (App uses 2s). */
  pollIntervalMs?: number;
}

/**
 * Watchdog loop for one workflow start.
 *
 * Returns once the bound workflow finishes, is superseded, or the
 * budget expires. Every write is gated on the binding, and the
 * timeout verdict only fires when the bound workflow is still busy —
 * so a loop that exits because B started can never complete or
 * time-out B (issue #26 defect 3).
 */
export async function runWorkflowWatchdog(options: WatchdogOptions): Promise<void> {
  const {
    binding,
    readSnapshot,
    fetchWorkflow,
    dispatch,
    sleep,
    now,
    timeoutMs,
    timeoutSummary,
    readWorkflowError,
    pollIntervalMs = 2000,
  } = options;

  const deadline = now() + timeoutMs;
  // Set when this loop itself dispatched a terminal state. Guards the
  // timeout verdict against firing on the normal completion path
  // (the reducer snapshot is still stale until React re-renders).
  let terminalDispatched = false;

  while (now() < deadline) {
    await sleep(pollIntervalMs);
    if (!shouldKeepPolling(binding, readSnapshot())) break;
    let reply: unknown;
    try {
      reply = await fetchWorkflow(binding.wfId);
    } catch {
      continue;
    }
    // Re-read AFTER the await: another workflow may have started
    // while this IPC was in flight.
    const outcome = decidePollOutcome(binding, readSnapshot(), reply);
    if (outcome.kind === "stale") return; // superseded: never touch the new run
    if (outcome.kind === "final-report") {
      dispatch({ type: "SET_FINAL_REPORT", report: outcome.summary });
      dispatch({ type: "SET_BUSY", value: false });
      dispatch({ type: "SET_COMPLETED", value: true });
      terminalDispatched = true;
      break;
    }
    if (outcome.kind === "mark-complete") {
      dispatch({ type: "SET_BUSY", value: false });
      dispatch({ type: "SET_COMPLETED", value: true });
      terminalDispatched = true;
      break;
    }
  }

  if (terminalDispatched) return;
  if (!shouldApplyTimeoutVerdict(binding, readSnapshot())) return;
  const error = readWorkflowError();
  dispatch({
    type: "SET_REVIEW_VERDICT",
    verdict: { verdict: "REPAIR", summary: error ?? timeoutSummary },
  });
  dispatch({ type: "SET_BUSY", value: false });
  dispatch({ type: "SET_COMPLETED", value: true });
}
