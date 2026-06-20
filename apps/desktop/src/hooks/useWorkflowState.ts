import { useEffect, useState } from 'react';

export interface WorkflowSummary {
  id: string;
  state: string;
  phase: string;
  finalStatus: 'DONE' | 'FAILED' | 'ABORTED' | null;
  createdAt: number;
  updatedAt: number;
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
        const { invoke } = await import('@tauri-apps/api/core');
        if (cancelled) return;
        const result = await invoke<WorkflowSummary | null>('get_workflow', { id: wfId });
        if (cancelled) return;
        setSummary(result);
      } catch (err) {
        // eslint-disable-next-line no-console
        console.warn('Tauri invoke unavailable:', err);
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
