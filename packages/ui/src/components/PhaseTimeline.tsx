import { cn } from '../lib/cn.js';

export type PhaseName =
  | 'requirement'
  | 'planning'
  | 'plan_review'
  | 'dispatch'
  | 'development'
  | 'review'
  | 'repair'
  | 'delivery';

export type PhaseState = 'pending' | 'active' | 'done' | 'failed';

export interface PhaseStep {
  name: PhaseName;
  state: PhaseState;
  label: string;
  durationMs?: number;
}

export interface PhaseTimelineProps {
  steps: readonly PhaseStep[];
  onStepClick?: (name: PhaseName) => void;
  className?: string;
}

const stateClass: Record<PhaseState, string> = {
  // No color blocks. State is communicated via border weight and
  // a tiny dot at the top of the bubble. v0.2: per user request,
  // status colors are reserved for task rows; the timeline stays
  // monochrome so it doesn't compete for attention with the
  // mission-control content.
  pending: 'border border-border text-text-secondary',
  active: 'border-2 border-chief text-primary ring-2 ring-chief/30',
  done: 'border border-text-secondary/60 text-text-secondary',
  failed: 'border border-status-failed text-status-failed',
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
              'hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-chief',
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
                {Math.round(s.durationMs / 1000)}秒
              </span>
            )}
          </button>
        </li>
      ))}
    </ol>
  );
}
