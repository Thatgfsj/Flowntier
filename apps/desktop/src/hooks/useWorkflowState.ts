import { useEffect, useState } from "react";

export interface WorkflowSummary {
  id: string;
  state: string;
  phase: string;
  finalStatus: "DONE" | "FAILED" | "ABORTED" | null;
  createdAt: number;
  updatedAt: number;
  // v0.4.22 (event 000118): the orchestrator's /api/workflow/{id}
  // response now also includes user_request (the original task text)
  // and a per-phase progress count. We expose them so the workbench
  // can show "what the task actually is" and "X / N tasks done".
  // Coalesced to empty string / 0 in useWorkflowState so consumers
  // don't have to handle `undefined`.
  userRequest: string;
  tasksDone: number;
  tasksTotal: number;
}

/**
 * Fetch and refresh the current workflow summary via TanStack Query.
 *
 * In v0.1 this is a thin wrapper; Phase 1 will switch to TanStack
 * Query for caching and revalidation.
 */
export function useWorkflowState(wfId: string | null): WorkflowSummary | null {
  const [summary, setSummary] = useState<WorkflowSummary | null>(null);

  useEffect(() => {
    if (wfId === null) {
      setSummary(null);
      return;
    }

    let cancelled = false;
    let timer: number | null = null;

    const fetchOnce = async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        if (cancelled) return;
        // The Tauri shell forwards to GET /api/workflow/{id} which
        // returns { ok, wf_id, status, phase, summary, user_request,
        // tasks_done, tasks_total }. Normalize to WorkflowSummary.
        const result = await invoke<{
          wf_id: string;
          status: string;
          phase: string;
          summary?: string;
          user_request?: string;
          tasks_done?: number;
          tasks_total?: number;
        } | null>("get_workflow", { id: wfId });
        if (cancelled) return;
        if (!result) {
          setSummary(null);
          return;
        }
        // v0.4.22 (event 000118): the orchestrator returns
        // optional `user_request`, `tasks_done`, `tasks_total`
        // — coalesce undefined to empty / 0 here so the
        // consumer sees a stable shape (exactOptionalPropertyTypes
        // rejects passing undefined into a `?: string` slot).
        const resAny = result as Record<string, unknown>;
        setSummary({
          id: (result.wf_id || resAny.id || wfId) as string,
          state: (result.status || resAny.state || "running") as string,
          phase: result.phase,
          finalStatus: null,
          createdAt: 0,
          updatedAt: 0,
          userRequest: (result.user_request ?? resAny.userRequest ?? "") as string,
          tasksDone: (result.tasks_done ?? resAny.tasksDone ?? 0) as number,
          tasksTotal: (result.tasks_total ?? resAny.tasksTotal ?? 0) as number,
        });
      } catch (err) {
        // eslint-disable-next-line no-console
        console.warn("Tauri invoke unavailable:", err);
      }
    };

    void fetchOnce();
    timer = window.setInterval(() => void fetchOnce(), 2_000);

    return () => {
      cancelled = true;
      if (timer !== null) window.clearInterval(timer);
    };
  }, [wfId]);

  return summary;
}
