import { cn } from '../lib/cn.js';

// v0.4.22 (event 000068): names match the orchestrator's
// 8-phase state machine (history/PROJECT_SPEC.md). Kept in
// sync with crates/pipe-server/src/orchestrator.rs::PHASES
// and apps/desktop/src/App.tsx::PHASES — any change must be
// applied to all three.
export type PhaseName =
  | 'requirement'
  | 'plan'
  | 'plan-review'
  | 'dispatch'
  | 'develop'
  | 'final-review'
  | 'repair'
  | 'delivery';

export type PhaseState = 'pending' | 'active' | 'done' | 'failed';

export interface PhaseStep {
  name: PhaseName;
  state: PhaseState;
  label: string;
  durationMs?: number;
  /** BUG-FRONTEND-RT-5 (event 000031): optional pre-formatted
   *  duration string (e.g. "12s" / "12 秒"). Falls back to a
   *  hardcoded `<n>秒` if not provided. */
  durationLabel?: string;
}

export interface PhaseTimelineProps {
  steps: readonly PhaseStep[];
  onStepClick?: (name: PhaseName) => void;
  className?: string;
}

const stateClass: Record<PhaseState, string> = {
  // v0.4.24 (event 000119): the timeline was monochrome and
  // indistinguishable from the panel behind it. Each phase state
  // now carries a faint tinted background so the eye can scan
  // "where are we in the pipeline" without reading labels.
  // Tints are low-alpha so they still feel mono when no phase is
  // active — only the active phase really glows.
  pending: 'border border-border bg-surface-3/40 text-text-secondary',
  active: 'border-2 border-chief bg-chief/15 text-primary',
  done: 'border border-status-done/60 bg-status-done/10 text-text-secondary',
  failed: 'border border-status-failed bg-status-failed/15 text-status-failed',
};

/**
 * Horizontal 8-step stepper. See `docs/UI_GUIDELINES.md` §3 T0.
 */
export function PhaseTimeline({ steps, onStepClick, className }: PhaseTimelineProps) {
  return (
    <ol
      className={cn('flex w-full items-center gap-1 overflow-x-auto p-2', className)}
      aria-label="工作流时间线"
    >
      {steps.map((s, i) => (
        <li key={s.name} className="flex-1 min-w-[100px]">
          <button
            type="button"
            onClick={() => onStepClick?.(s.name)}
            className={cn(
              'flex w-full flex-col items-center gap-1 rounded-md p-2 text-xs transition-colors',
              'hover:bg-surface-3/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-chief',
              // v0.4.24 (event 000119): when a phase is active, the
              // whole button gets the chief glow so the eye snaps
              // to "the orchestrator is here" without reading text.
              s.state === 'active' && 'flt-glow-chief',
            )}
            aria-label={`${s.label} (${s.state})`}
          >
            <span
              className={cn(
                'flex h-7 w-7 items-center justify-center rounded-full text-[11px] font-semibold tabular-nums',
                stateClass[s.state],
              )}
            >
              {i + 1}
            </span>
            <span className="truncate text-center">{s.label}</span>
            {s.durationMs !== undefined && (
              <span className="text-[10px] text-text-secondary tabular-nums">
                {s.durationLabel ?? `${Math.round(s.durationMs / 1000)}秒`}
              </span>
            )}
          </button>
        </li>
      ))}
    </ol>
  );
}
