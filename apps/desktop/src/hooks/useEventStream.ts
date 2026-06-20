import { useEffect, useState } from 'react';
import type { WfEvent } from '@aco/shared';

/**
 * Subscribe to the ACO event stream. Tauri v2 ships events via the
 * `wf:event` channel (see `crates/tauri-core/src/lib.rs`).
 *
 * Returns a list of events as they arrive. Newest at the end.
 */
export function useEventStream(): WfEvent[] {
  const [events, setEvents] = useState<WfEvent[]>([]);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;

    void (async () => {
      try {
        // Dynamic import to avoid loading Tauri APIs in non-Tauri
        // environments (e.g. Storybook, tests).
        const { listen } = await import('@tauri-apps/api/event');
        if (cancelled) return;
        const off = await listen<WfEvent>('wf:event', (e) => {
          setEvents((prev) => [...prev, e.payload]);
        });
        unlisten = off;
      } catch (err) {
        // Not running under Tauri (e.g. browser-only mode).
        // eslint-disable-next-line no-console
        console.warn('Tauri event API unavailable:', err);
      }
    })();

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  return events;
}
