/**
 * Pure helper extracted from ChatZone so we can unit-test the
 * phase-8 fallback rule. v0.4.22 (event 000118, fix 5):
 *
 *   "If the in-flight `text` accumulator is empty AND the
 *    latest `done` event carries a non-empty summary, render
 *    the summary instead of an empty transcript."
 *
 * Why: chief phase 8 emits one ~1185-char SSE response in
 * ~5s; React batches TextDeltas into a single render, so
 * the user perceives "ChatZone 没动静". Falling back to
 * the Done.summary keeps the report visible.
 *
 * The actual `useMemo` in ChatZone just calls this. We keep
 * the function pure (events: readonly shape, text: string)
 * so the vitest can drive it without React.
 *
 * We type `events` as `ReadonlyArray<{ kind: string; summary?: unknown }>`
 * to accept BOTH the shared `WfEvent` (which has stricter
 * per-variant shape) AND the runtime `AgentEvent[]` that
 * `useAgentStream` returns (which uses the raw serde
 * `AgentEvent` shape — the two diverged in event 000116
 * cleanup, see shared/src/events.ts for the history).
 */
export type FallbackEvent = { readonly kind: string; readonly summary?: unknown };

/**
 * Returns the summary to render in place of an empty transcript,
 * or `null` if no fallback is needed.
 *
 * @param events  All events seen in the current run (order
 *                doesn't matter — we look for the latest `done`).
 * @param text    The accumulated text from text_delta events so
 *                far. If non-empty, we return `null` (streaming
 *                text always wins).
 */
export function fallbackSummary(events: readonly FallbackEvent[], text: string): string | null {
  if (text.length > 0) return null;

  // Walk events in REVERSE to find the latest `done`.
  for (let i = events.length - 1; i >= 0; i--) {
    const e = events[i];
    if (!e || e.kind !== "done") continue;
    const summary = e.summary;
    if (typeof summary === "string" && summary.trim().length > 0) {
      return summary;
    }
    // Found a done but no usable summary — return null because
    // an earlier (older) `done` would be from a previous run
    // and is no longer relevant.
    return null;
  }

  return null;
}
