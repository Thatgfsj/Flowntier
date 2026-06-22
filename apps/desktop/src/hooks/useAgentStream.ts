/**
 * Subscribe to ACO agent events emitted by the embedded Rust
 * agent loop (see `crates/agent-core/src/event.rs`).
 *
 * The Tauri backend forwards every `AgentEvent` over the same
 * `wf:event` channel as workflow events (so we reuse the
 * existing Tauri listener path); the discriminator is the
 * `kind` field, not the event class.
 *
 * Returns a chronologically-ordered log of agent events for
 * the *current* chat session. A `reset()` helper starts a new
 * session.
 */
import { useCallback, useEffect, useRef, useState } from 'react';

export type AgentEvent =
  | { kind: 'text_delta'; agent_id: string; agent_display: string; delta: string }
  | { kind: 'tool_started'; agent_id: string; agent_display: string; call: { id: string; name: string; args: unknown } }
  | { kind: 'tool_finished'; agent_id: string; agent_display: string; tool_call_id: string; preview: string; is_error: boolean; elapsed_ms: number }
  | { kind: 'phase_transition'; wf_id: string; from: string | null; to: string }
  | { kind: 'token_usage'; agent_id: string; provider: string; model: string; input_tokens: number; output_tokens: number; cost_usd: number | null }
  | { kind: 'done'; wf_id: string; status: string; summary: string | null };

const AGENT_KINDS: ReadonlySet<string> = new Set([
  'text_delta',
  'tool_started',
  'tool_finished',
  'phase_transition',
  'token_usage',
  'done',
]);

function isAgentEvent(payload: unknown): payload is AgentEvent {
  if (typeof payload !== 'object' || payload === null) return false;
  const k = (payload as { kind?: string }).kind;
  return typeof k === 'string' && AGENT_KINDS.has(k);
}

export interface UseAgentStreamResult {
  /** Chronological event log for the active session. */
  events: AgentEvent[];
  /** Concatenated assistant text for the active turn. */
  text: string;
  /** True once a `done` event has arrived. */
  done: boolean;
  /** Terminal status string from the last `done` event. */
  status: string | null;
  /** Clear log + start a fresh session. */
  reset: () => void;
}

export function useAgentStream(): UseAgentStreamResult {
  const [events, setEvents] = useState<AgentEvent[]>([]);
  const [text, setText] = useState('');
  const [done, setDone] = useState(false);
  const [status, setStatus] = useState<string | null>(null);

  // Use a ref so the listener closure always sees the latest setter.
  const setEventsRef = useRef(setEvents);
  setEventsRef.current = setEvents;

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;

    void (async () => {
      try {
        const { listen } = await import('@tauri-apps/api/event');
        if (cancelled) return;
        const off = await listen<unknown>('wf:event', (e) => {
          if (!isAgentEvent(e.payload)) return;
          const ev = e.payload;
          setEventsRef.current((prev) => [...prev, ev]);
          if (ev.kind === 'text_delta') {
            setText((t) => t + ev.delta);
          } else if (ev.kind === 'done') {
            setDone(true);
            setStatus(ev.status);
          } else if (ev.kind === 'tool_finished') {
            // After a tool finishes, a fresh assistant turn may follow.
            // We do not clear `text` here — the next text_delta will
            // continue to append, which matches the user's mental model.
          }
        });
        unlisten = off;
      } catch (err) {
        // Not running under Tauri — silent no-op.
        // eslint-disable-next-line no-console
        console.warn('Tauri event API unavailable for useAgentStream:', err);
      }
    })();

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  const reset = useCallback(() => {
    setEvents([]);
    setText('');
    setDone(false);
    setStatus(null);
  }, []);

  return { events, text, done, status, reset };
}
