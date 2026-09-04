// apps/desktop/src/contexts/workflowReducer.ts
//
// v0.4.29: lifted the 32 useState calls + applyEvent switch from
// App.tsx into a single pure reducer. The state shape mirrors
// the previous field-for-field so callers don't have to change
// anything except how they READ it (via `useWorkflow()` /
// selectors in `WorkflowContext.tsx`).
//
// Architecture:
//
//   ┌──────────────┐    ┌──────────────────┐    ┌──────────────┐
//   │ useEventStream│ → │ dispatch(event)  │ → │  reducer()   │ → state
//   └──────────────┘    └──────────────────┘    └──────────────┘
//
//   ┌──────────────────────────────────────────────────────────┐
//   │ TICK (setInterval)         → flip stale roles to idle    │
//   │ RESET                      → clear workflow-only state   │
//   │ START_WORKFLOW             → busy=true, clear previous   │
//   │ SET_CURRENT_WF_ID          → for polling                 │
//   │ SET_FINAL_REPORT (poll)    → for the 30-min watchdog     │
//   │ SET_REVIEW_VERDICT_TIMEOUT → watchdog fired w/o error    │
//   │ SET_REVIEW_VERDICT (err)   → watchdog fired with error   │
//   │ SET_REVIEW_VERDICT (fail)  → workflow start threw        │
//   └──────────────────────────────────────────────────────────┘

import type { AgentStatus, PhaseState } from "@flowntier/ui";
import type { ReviewerVerdictEvent, WfEvent } from "@flowntier/shared";
import { agentIdToHeadRole } from "../lib/agentId.js";

// ── Phase table (matches App.tsx PHASES) ──────────────────────────
// Lives here so the reducer is the sole consumer of phase names.
// Kept in lockstep with `crates/pipe-server/src/orchestrator.rs::PHASES`.

export type PhaseName =
  | "requirement"
  | "plan"
  | "plan-review"
  | "dispatch"
  | "develop"
  | "final-review"
  | "repair"
  | "delivery";

export interface PhaseDef {
  readonly name: PhaseName;
  readonly label: string;
}

export const PHASES: ReadonlyArray<PhaseDef> = [
  { name: "requirement", label: "1-需求" },
  { name: "plan", label: "2-规划" },
  { name: "plan-review", label: "3-计划审核" },
  { name: "dispatch", label: "4-派发" },
  { name: "develop", label: "5-开发" },
  { name: "final-review", label: "6-终审" },
  { name: "repair", label: "7-修复" },
  { name: "delivery", label: "8-交付" },
];

export const PHASE_STATE_INITIAL: Record<PhaseName, PhaseState> = {
  requirement: "pending",
  plan: "pending",
  "plan-review": "pending",
  dispatch: "pending",
  develop: "pending",
  "final-review": "pending",
  repair: "pending",
  delivery: "pending",
};

// ── Domain shapes ─────────────────────────────────────────────────

export interface AgentStatusEntry {
  status: AgentStatus;
  /** Wall-clock ms when this status was last set. The TICK action
   *  uses `now - since > IDLE_TIMEOUT_MS` to auto-flip to idle. */
  since: number;
}

export type AgentStatusMap = {
  chief: AgentStatusEntry;
  "critic-a": AgentStatusEntry;
  "critic-b": AgentStatusEntry;
  worker: AgentStatusEntry;
};

export const INITIAL_AGENT_STATUS: AgentStatusMap = {
  chief: { status: "idle", since: 0 },
  "critic-a": { status: "idle", since: 0 },
  "critic-b": { status: "idle", since: 0 },
  worker: { status: "idle", since: 0 },
};

export interface TaskRow {
  id: string;
  title: string;
  owner: string;
  fileHint?: string;
  state: string;
  summary?: string;
}

export interface ReviewVerdictState {
  verdict: "PASS" | "REPAIR" | "REWRITE";
  summary: string;
}

export interface DriftState {
  detected: boolean;
  sidecar?: string;
  minCompatible?: string;
}

// ── Workflow-only state (resets on every workflow start) ──────────

export interface WorkflowState {
  // workflow lifecycle
  busy: boolean;
  completed: boolean;
  currentWfId: string | null;
  cancelling: boolean;

  // phase machine
  activePhase: number;
  phaseStates: Record<PhaseName, PhaseState>;

  // tasks
  tasks: TaskRow[];

  // agents
  agentStatus: AgentStatusMap;
  workerTaskStatus: Record<string, AgentStatus>;
  workerTaskTitles: Record<string, string>;

  // event log (append-only; RightPanel / BottomConsole need this)
  events: WfEvent[];

  // milestones
  milestones: string[];

  // reviewer output
  reviewVerdict: ReviewVerdictState | null;
  reviewerVerdicts: ReviewerVerdictEvent[];
  finalReport: string | null;
  workflowError: string | null;
}

// ── App-only state (persists across workflow runs) ────────────────

export interface AppState {
  // first-run gate
  firstRun: boolean | null;

  // workspace
  workdir: string | null;
  workdirReady: boolean;
  workdirSkipped: boolean;

  // settings
  settingsOpen: boolean;
  chatOpen: boolean;

  // updates
  updateBanner: { available: boolean };

  // sidecar drift
  drift: DriftState;
}

export type RootState = WorkflowState & AppState;

// ── Initial state ─────────────────────────────────────────────────

export const INITIAL_STATE: RootState = {
  // workflow
  busy: false,
  completed: false,
  currentWfId: null,
  cancelling: false,
  activePhase: 0,
  phaseStates: { ...PHASE_STATE_INITIAL },
  tasks: [],
  agentStatus: cloneStatusMap(INITIAL_AGENT_STATUS),
  workerTaskStatus: {},
  workerTaskTitles: {},
  events: [],
  milestones: [],
  reviewVerdict: null,
  reviewerVerdicts: [],
  finalReport: null,
  workflowError: null,

  // app
  firstRun: null,
  workdir: null,
  workdirReady: false,
  workdirSkipped: false,
  settingsOpen: false,
  chatOpen: false,
  updateBanner: { available: false },
  drift: { detected: false },
};

function cloneStatusMap(m: AgentStatusMap): AgentStatusMap {
  return {
    chief: { ...m.chief },
    "critic-a": { ...m["critic-a"] },
    "critic-b": { ...m["critic-b"] },
    worker: { ...m.worker },
  };
}

const IDLE_TIMEOUT_MS = 15_000;

// ── Actions ───────────────────────────────────────────────────────

export type WfAction =
  | { type: "EVENT"; event: WfEvent }
  | { type: "TICK"; now: number }
  | { type: "RESET" }
  | { type: "START_WORKFLOW" }
  | { type: "SET_CURRENT_WF_ID"; id: string | null }
  | { type: "SET_ACTIVE_PHASE"; index: number }
  | { type: "SET_CANCELLING"; value: boolean }
  | { type: "SET_BUSY"; value: boolean }
  | { type: "SET_COMPLETED"; value: boolean }
  | { type: "SET_FINAL_REPORT"; report: string }
  | { type: "SET_REVIEW_VERDICT"; verdict: ReviewVerdictState }
  | { type: "SET_FIRST_RUN"; value: boolean | null }
  | { type: "SET_WORKDIR"; value: string | null }
  | { type: "SET_WORKDIR_READY" }
  | { type: "SET_WORKDIR_SKIPPED"; value: boolean }
  | { type: "SET_SETTINGS_OPEN"; value: boolean }
  | { type: "SET_CHAT_OPEN"; value: boolean }
  | { type: "SET_UPDATE_BANNER"; value: { available: boolean } }
  | { type: "SET_DRIFT"; value: DriftState }
  | { type: "SET_WORKFLOW_ERROR"; error: string | null };

// ── Reducer ───────────────────────────────────────────────────────

export function workflowReducer(state: RootState, action: WfAction): RootState {
  switch (action.type) {
    case "TICK": {
      // Flip any non-idle role back to idle if it hasn't been
      // touched in the last IDLE_TIMEOUT_MS. Without this, chief
      // cards would stay "thinking" forever after the LLM stops
      // streaming (no terminal event is fired per-stream).
      const next = { ...state };
      const ages = cloneStatusMap(state.agentStatus);
      let dirty = false;
      for (const k of ["chief", "critic-a", "critic-b", "worker"] as const) {
        const e = ages[k];
        if (e.status !== "idle" && action.now - e.since > IDLE_TIMEOUT_MS) {
          ages[k] = { status: "idle", since: action.now };
          dirty = true;
        }
      }
      if (dirty) next.agentStatus = ages;
      return next;
    }

    case "RESET": {
      // Match the previous App.tsx `reset()` semantics: clears
      // workflow-only state but does NOT touch `busy` — the caller
      // flips `busy=false` via SET_BUSY in the same user flow.
      // This keeps the spinner going until the new workflow
      // actually starts (preventing a flash of "idle" between
      // the user clicking "Reset" and dispatching START_WORKFLOW).
      return {
        ...state,
        activePhase: 0,
        phaseStates: { ...PHASE_STATE_INITIAL },
        tasks: [],
        agentStatus: cloneStatusMap(INITIAL_AGENT_STATUS),
        events: [],
        completed: false,
        milestones: [],
        reviewVerdict: null,
        reviewerVerdicts: [],
        finalReport: null,
        workflowError: null,
        currentWfId: null,
      };
    }

    case "START_WORKFLOW": {
      return {
        ...state,
        busy: true,
        completed: false,
        events: [],
        tasks: [],
        milestones: [],
        reviewVerdict: null,
        reviewerVerdicts: [],
        finalReport: null,
        workflowError: null,
        workerTaskStatus: {},
        workerTaskTitles: {},
        activePhase: 0,
        phaseStates: { ...PHASE_STATE_INITIAL },
        agentStatus: cloneStatusMap(INITIAL_AGENT_STATUS),
      };
    }

    case "SET_CURRENT_WF_ID":
      return { ...state, currentWfId: action.id };
    case "SET_ACTIVE_PHASE":
      return { ...state, activePhase: action.index };
    case "SET_CANCELLING":
      return { ...state, cancelling: action.value };
    case "SET_BUSY":
      return { ...state, busy: action.value };
    case "SET_COMPLETED":
      return { ...state, completed: action.value };
    case "SET_FINAL_REPORT":
      return { ...state, finalReport: action.report };
    case "SET_REVIEW_VERDICT":
      return { ...state, reviewVerdict: action.verdict };
    case "SET_FIRST_RUN":
      return { ...state, firstRun: action.value };
    case "SET_WORKDIR":
      return { ...state, workdir: action.value };
    case "SET_WORKDIR_READY":
      return { ...state, workdirReady: true };
    case "SET_WORKDIR_SKIPPED":
      return { ...state, workdirSkipped: action.value };
    case "SET_SETTINGS_OPEN":
      return { ...state, settingsOpen: action.value };
    case "SET_CHAT_OPEN":
      return { ...state, chatOpen: action.value };
    case "SET_UPDATE_BANNER":
      return { ...state, updateBanner: action.value };
    case "SET_DRIFT":
      return { ...state, drift: action.value };
    case "SET_WORKFLOW_ERROR":
      return { ...state, workflowError: action.error };

    case "EVENT": {
      return reduceEvent(state, action.event);
    }
  }
}

// ── Per-event reducer (kept as a separate fn for readability) ─────

function reduceEvent(state: RootState, event: WfEvent): RootState {
  const now = Date.now();
  let next: RootState = {
    ...state,
    events: [...state.events, event],
  };

  // ── reviewer_verdict (always first — feeds final-review binding)
  if (event.kind === "reviewer_verdict") {
    const r = event as ReviewerVerdictEvent;
    next = {
      ...next,
      reviewerVerdicts: [...next.reviewerVerdicts, r],
    };
    if (r.phase === "final-review") {
      const v = r.verdict;
      const coerced: "PASS" | "REPAIR" | "REWRITE" =
        v === "PASS" || v === "REPAIR" || v === "REWRITE" ? v : "REPAIR";
      next = {
        ...next,
        reviewVerdict: { verdict: coerced, summary: r.summary },
      };
    }
  }

  // ── done (terminal)
  if (event.kind === "done") {
    const status = event.status;
    const aborted = status.startsWith("ABORTED") || status.startsWith("FAILED");
    if (status.startsWith("FAILED")) {
      const summary = event.summary ?? "";
      const clean = status.replace(/^FAILED:\s*/, "");
      next = {
        ...next,
        workflowError: `${clean}\n\n${summary}`.trim(),
      };
    }
    if (aborted && next.busy) {
      next = {
        ...next,
        busy: false,
        completed: true,
        finalReport: event.summary ?? (status.startsWith("ABORTED") ? "已中止" : "工作流失败"),
      };
    } else if (next.busy) {
      // Guard against premature intermediate subtask done: only complete workflow
      // if we are in the delivery phase (phase 7) OR the delivery summary marker is present
      const isDeliveryPhase =
        next.activePhase === 7 || (event.summary && event.summary.includes("[Final Review]"));
      if (isDeliveryPhase) {
        next = {
          ...next,
          busy: false,
          completed: true,
          finalReport: event.summary ?? next.finalReport ?? "工作流完成",
          reviewVerdict: next.reviewVerdict ?? {
            verdict: "PASS",
            summary: event.summary ?? "通过",
          },
        };
      }
    }
  }

  // ── transition or phase_transition (compatibility for runtime events)
  if (
    (event.kind === "transition" || event.kind === "phase_transition") &&
    (event as { to?: string }).to
  ) {
    const to = (event as { to: string }).to;
    const idx = PHASES.findIndex((p) => p.name === to);
    if (idx >= 0) {
      const phaseStates = { ...next.phaseStates };
      for (let i = 0; i < idx; i++) {
        const name = PHASES[i]?.name;
        if (name) phaseStates[name] = "done";
      }
      const toName = to as PhaseName;
      phaseStates[toName] = "active";
      next = { ...next, activePhase: idx, phaseStates };

      const role = phaseToRole(to);
      if (role) {
        const agentStatus = cloneStatusMap(next.agentStatus);
        agentStatus[role] = { status: "thinking", since: now };
        next = { ...next, agentStatus };
      }
    }
  }

  // ── repair_loop
  if (event.kind === "repair_loop") {
    const loopIdx = event.loop_index ?? 1;
    const maxLoops = event.max_loops ?? 3;
    const label = `修复循环 ${loopIdx}/${maxLoops}: A(${event.verdict_a ?? "REPAIR"}), B(${event.verdict_b ?? "REPAIR"})`;
    next = {
      ...next,
      activePhase: 5,
      milestones: [...next.milestones, label],
    };
  }

  // ── milestone.label
  if (event.kind === "milestone" && event.label) {
    next = { ...next, milestones: [...next.milestones, event.label] };
  }

  // ── milestone.phase
  if (event.kind === "milestone" && event.phase) {
    const phaseName = event.phase as PhaseName;
    const idx = PHASES.findIndex((p) => p.name === phaseName);
    if (idx >= 0) {
      const ms = event as { status?: string };
      const newState: PhaseState =
        ms.status === "completed"
          ? "done"
          : ms.status === "started" || ms.status === "in_progress"
            ? "active"
            : "pending";
      const phaseStates = { ...next.phaseStates, [phaseName]: newState };
      next = {
        ...next,
        activePhase: newState === "active" ? idx : next.activePhase,
        phaseStates,
      };

      const role = phaseToRole(phaseName);
      if (role) {
        const agentStatus = cloneStatusMap(next.agentStatus);
        agentStatus[role] = {
          status: newState === "done" ? "idle" : "thinking",
          since: now,
        };
        next = { ...next, agentStatus };
      }

      // Completion: last milestone = delivery + completed → finish.
      if (phaseName === "delivery" && ms.status === "completed" && !next.completed) {
        next = {
          ...next,
          busy: false,
          completed: true,
          reviewVerdict: next.reviewVerdict ?? { verdict: "PASS", summary: "通过" },
        };
      }
    }
  }

  // ── text_delta (drives per-role speaking status for ALL roles,
  //    not just workers-with-task-id like the previous code did)
  if (event.kind === "text_delta") {
    const taskId = event.task_id;
    if (taskId) {
      const workerTaskStatus = { ...next.workerTaskStatus };
      workerTaskStatus[taskId] = "speaking";
      next = { ...next, workerTaskStatus };
    }
    const role = agentIdToHeadRole(event.agent_id);
    if (role) {
      const agentStatus = cloneStatusMap(next.agentStatus);
      agentStatus[role] = { status: "speaking", since: now };
      next = { ...next, agentStatus };
    }
  }

  // ── tool_started
  if (event.kind === "tool_started") {
    const taskId = event.task_id;
    if (taskId) {
      const workerTaskStatus = { ...next.workerTaskStatus };
      workerTaskStatus[taskId] = "thinking";
      next = { ...next, workerTaskStatus };
    }
    const role = agentIdToHeadRole(event.agent_id);
    if (role) {
      const agentStatus = cloneStatusMap(next.agentStatus);
      agentStatus[role] = { status: "thinking", since: now };
      next = { ...next, agentStatus };
    }
  }

  // ── tool_finished
  if (event.kind === "tool_finished") {
    const taskId = event.task_id;
    if (taskId) {
      const workerTaskStatus = { ...next.workerTaskStatus };
      workerTaskStatus[taskId] = "speaking";
      next = { ...next, workerTaskStatus };
    }
    const role = agentIdToHeadRole(event.agent_id);
    if (role) {
      const agentStatus = cloneStatusMap(next.agentStatus);
      agentStatus[role] = { status: "speaking", since: now };
      next = { ...next, agentStatus };
    }
  }

  // ── console (still flips the role to thinking even though
  //    text_delta / tool_* now drive the real-time status)
  if (event.kind === "console" && event.agent_id) {
    const role = agentIdToHeadRole(event.agent_id);
    if (role) {
      const agentStatus = cloneStatusMap(next.agentStatus);
      agentStatus[role] = { status: "thinking", since: now };
      next = { ...next, agentStatus };
    }
  }

  // ── task_status
  if (event.kind === "task_status" && event.task_id) {
    const newState = event.task_status;
    const summary = event.task_summary;
    if (event.task_title) {
      const workerTaskTitles = { ...next.workerTaskTitles };
      workerTaskTitles[event.task_id] = event.task_title;
      next = { ...next, workerTaskTitles };
    }
    const tasks = upsertTask(next.tasks, {
      id: event.task_id,
      title: event.task_title ?? event.task_id,
      owner: "",
      state: newState,
      ...(summary !== undefined ? { summary } : {}),
    });
    next = { ...next, tasks };
  }

  return next;
}

function upsertTask(tasks: TaskRow[], fresh: TaskRow): TaskRow[] {
  const idx = tasks.findIndex((t) => t.id === fresh.id);
  if (idx < 0) return [...tasks, fresh];
  const next = tasks.slice();
  const existing = next[idx];
  if (existing) {
    // Keep `summary: undefined` from creeping into the
    // existing record — RightPanelTask's `summary?: string`
    // (with exactOptionalPropertyTypes) rejects an explicit
    // `undefined`.
    const merged: TaskRow =
      fresh.summary !== undefined
        ? { ...existing, state: fresh.state, summary: fresh.summary }
        : { ...existing, state: fresh.state };
    next[idx] = merged;
  }
  return next;
}

// ── phase → role mapping (preserved verbatim from App.tsx) ────────

export type RoleKey = keyof AgentStatusMap;

export function phaseToRole(phase: string): RoleKey | null {
  switch (phase) {
    case "requirement":
    case "plan":
    case "dispatch":
    case "repair":
    case "delivery":
      return "chief";
    case "plan-review":
    case "final-review":
      return "critic-a";
    case "develop":
      return "worker";
    default:
      return null;
  }
}
