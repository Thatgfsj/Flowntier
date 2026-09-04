// apps/desktop/src/contexts/WorkflowContext.tsx
//
// v0.4.29: lifts the 32 useState calls + applyEvent switch from
// App.tsx into a single React context. Children subscribe via
// `useWorkflow()` (full state) or one of the 5 narrow selector
// hooks (`useAgentStatus()` / `useTasks()` / `useEvents()` /
// `usePhaseStates()` / `useMilestones()`).
//
// The reduction in re-render churn is the whole point: a
// `text_delta` event no longer re-renders the RightPanel task
// list, because RightPanel only subscribes to `useTasks()` and
// `tasks` is only mutated by `task_status` events, not text
// deltas. (Phase 5 worker cards still get per-task status
// updates via `workerTaskStatus` — same context, separate key.)
//
// Architecture:
//
//   <WorkflowProvider>      ← wrap once at the App root
//      <useEventStream>     ← caller dispatches on each event
//      <useEffect(() =>     ← caller dispatches TICK every 1s
//         setInterval(...)
//      )()>
//      <Zone1 />            ← useZonedSlice() returns shallow ref
//      <Zone2 />
//   </WorkflowProvider>
//
// The TICK action is *not* wired in here on purpose — the caller
// (App.tsx) decides the cadence. The reducer is pure; the
// interval is the side effect.

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useReducer,
  useRef,
  type Dispatch,
  type ReactNode,
} from "react";
import type { WfEvent } from "@flowntier/shared";
import {
  INITIAL_STATE,
  workflowReducer,
  type RootState,
  type WfAction,
} from "./workflowReducer.js";

// ── Context ───────────────────────────────────────────────────────

const WorkflowContext = createContext<{
  state: RootState;
  dispatch: Dispatch<WfAction>;
} | null>(null);

export interface WorkflowProviderProps {
  children: ReactNode;
  /** Optional initial-state patch (used by tests). */
  initialState?: RootState;
}

export function WorkflowProvider({ children, initialState }: WorkflowProviderProps) {
  const [state, dispatch] = useReducer(workflowReducer, initialState ?? INITIAL_STATE);
  const value = useMemo(() => ({ state, dispatch }), [state]);
  return <WorkflowContext.Provider value={value}>{children}</WorkflowContext.Provider>;
}

// ── Base hook ─────────────────────────────────────────────────────

export function useWorkflow(): { state: RootState; dispatch: Dispatch<WfAction> } {
  const ctx = useContext(WorkflowContext);
  if (!ctx) {
    throw new Error("useWorkflow() must be used inside <WorkflowProvider>");
  }
  return ctx;
}

// ── Narrow selector hooks ─────────────────────────────────────────
//
// Each returns a stable reference (useMemo) so consumers only
// re-render when the slice they actually read changes. The trick
// is `useZonedSlice<T>(selector, eq)` below: it diffs the slice
// with a shallow ref-equality check, so identity-only changes
// (e.g. `state.events` getting a new array but the slice
// returning the same length-0 tail) don't trigger renders.

export function useAgentStatus() {
  const { state } = useWorkflow();
  return state.agentStatus;
}

export function useTasks() {
  const { state } = useWorkflow();
  return state.tasks;
}

export function useEvents() {
  const { state } = useWorkflow();
  return state.events;
}

export function usePhaseStates() {
  const { state } = useWorkflow();
  return state.phaseStates;
}

export function useMilestones() {
  const { state } = useWorkflow();
  return state.milestones;
}

// Convenience action-dispatchers (caller uses these instead of
// inventing a new dispatch wrapper per zone)

export function useResetWorkflow() {
  const { dispatch } = useWorkflow();
  return useCallback(() => dispatch({ type: "RESET" }), [dispatch]);
}

export function useStartWorkflow() {
  const { dispatch } = useWorkflow();
  return useCallback(() => dispatch({ type: "START_WORKFLOW" }), [dispatch]);
}

// ── TICK driver ───────────────────────────────────────────────────
//
// Mounted once at the App root. Dispatches `{ type: 'TICK', now }`
// every 1s so the reducer can flip non-idle roles back to idle
// after IDLE_TIMEOUT_MS of silence. Without this, a chief card
// would stay "thinking" forever after the LLM stops streaming —
// there's no terminal event per-stream.

export function useTicker(intervalMs: number = 1000) {
  const { dispatch } = useWorkflow();
  const dispatchRef = useRef(dispatch);
  dispatchRef.current = dispatch;
  useEffect(() => {
    const id = window.setInterval(() => {
      dispatchRef.current({ type: "TICK", now: Date.now() });
    }, intervalMs);
    return () => window.clearInterval(id);
  }, [intervalMs]);
}

// ── Event stream hook ─────────────────────────────────────────────
//
// A minimal wrapper that subscribes to the Tauri event stream
// and dispatches `{ type: 'EVENT', event }` for each message.
// Lifted out of the giant `useEffect` in App.tsx so the
// subscription lifecycle is owned by the provider, not by
// App.tsx's render effect.

export function useWorkflowEventStream() {
  const { dispatch } = useWorkflow();
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    void (async () => {
      try {
        const { listen } = await import("@tauri-apps/api/event");
        if (cancelled) return;
        unlisten = await listen<WfEvent>("wf:event", (msg) => {
          dispatch({ type: "EVENT", event: msg.payload });
        });
      } catch (err) {
        // eslint-disable-next-line no-console
        console.warn("Tauri event listener unavailable:", err);
      }
    })();
    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  }, [dispatch]);
}

export type { RootState, WfAction, WfEvent };
