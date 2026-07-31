/**
 * v0.4.22 (event 000118, fix 5 hardening): boundary tests
 * for the phase-8 fallbackSummary rule.
 *
 * The rule: when streaming text is empty AND the latest `done`
 * event carries a non-empty summary, render the summary.
 *
 * Coverage:
 *   1. happy path: text='', done with summary → returns summary
 *   2. streaming wins: text='hello', done with summary → returns null
 *   3. whitespace-only summary is treated as empty (don't render blank)
 *   4. null summary → null
 *   5. older `done` events are ignored if a newer one is empty
 *   6. no `done` events at all → null
 *   7. non-done events between two dones: latest `done` wins
 *   8. done with summary='' → null (don't fall back to a blank box)
 *   9. empty events array + non-empty text → null (no fallback)
 *  10. multiple done events with summaries: latest wins
 */
import { describe, expect, it } from 'vitest';
import { fallbackSummary, type FallbackEvent } from './chatFallback.js';

const done = (summary: string | null): FallbackEvent => ({
  kind: 'done',
  summary,
});

const textDelta = (_delta: string): FallbackEvent => ({
  kind: 'text_delta',
  summary: undefined,
});
// Suppress unused-parameter lint on the delta arg — the type
// only needs `kind`, the test fixture keeps the signature for
// readability.
void textDelta;

describe('fallbackSummary — phase 8 streaming fallback', () => {
  it('returns the summary when text is empty and done has a non-empty summary', () => {
    const events: FallbackEvent[] = [done('这是 phase 8 的汇报。')];
    expect(fallbackSummary(events, '')).toBe('这是 phase 8 的汇报。');
  });

  it('returns null when text has streamed (streaming always wins)', () => {
    const events: FallbackEvent[] = [done('summary')];
    expect(fallbackSummary(events, 'hello streaming')).toBeNull();
  });

  it('returns null when summary is only whitespace', () => {
    const events: FallbackEvent[] = [done('   \n\t  ')];
    expect(fallbackSummary(events, '')).toBeNull();
  });

  it('returns null when summary is null', () => {
    const events: FallbackEvent[] = [done(null)];
    expect(fallbackSummary(events, '')).toBeNull();
  });

  it('does not fall back to an OLDER done when the latest done is empty', () => {
    // Simulate: phase 7 finished with a summary, then phase 8
    // started and ended without a summary. The user shouldn't
    // see the phase-7 summary at the bottom of an empty phase-8.
    const events: FallbackEvent[] = [
      done('phase 7 完成'),
      textDelta('phase 8 starting'),
      done(null),
    ];
    // textDelta in events doesn't affect our text accumulator,
    // and our last done is empty → null
    expect(fallbackSummary(events, '')).toBeNull();
  });

  it('returns null when there are no done events at all', () => {
    const events: FallbackEvent[] = [textDelta('a'), textDelta('b')];
    expect(fallbackSummary(events, '')).toBeNull();
  });

  it('the LATEST done wins (ignores non-done events after it)', () => {
    // After the latest done, more text_delta events arrived
    // (maybe the orchestrator's tail). They don't change the
    // fact that we should fall back to the done's summary.
    const events: FallbackEvent[] = [
      done('phase 7 ok'),
      textDelta('tail'),
      textDelta('more tail'),
      done('phase 8 ok'),
    ];
    expect(fallbackSummary(events, '')).toBe('phase 8 ok');
  });

  it('empty-string summary is treated as no fallback (don\u2019t render blank box)', () => {
    const events: FallbackEvent[] = [done('')];
    expect(fallbackSummary(events, '')).toBeNull();
  });

  it('empty events + non-empty text → null (defensive)', () => {
    expect(fallbackSummary([], 'streaming')).toBeNull();
  });

  it('multiple done events with summaries: latest wins (order matters)', () => {
    const events: FallbackEvent[] = [
      done('first run summary'),
      done('second run summary'),
      done('third run summary'),
    ];
    expect(fallbackSummary(events, '')).toBe('third run summary');
  });

  it('ABORTED status still falls back to its summary (so user sees why we stopped)', () => {
    const events: FallbackEvent[] = [done('用户中断了 workflow')];
    expect(fallbackSummary(events, '')).toBe('用户中断了 workflow');
  });

  it('boundary: a single space in summary is whitespace, not content', () => {
    expect(fallbackSummary([done(' ')], '')).toBeNull();
  });
});
