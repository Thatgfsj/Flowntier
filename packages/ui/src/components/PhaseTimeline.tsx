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

/**
 * Modern connected horizontal pipeline stepper.
 */
export function PhaseTimeline({ steps, onStepClick, className }: PhaseTimelineProps) {
  return (
    <nav
      className={cn(
        'flex w-full items-center rounded-xl border border-white/8 bg-surface-1/90 p-2.5 shadow-sm backdrop-blur-md',
        className,
      )}
      aria-label="工作流流水线"
    >
      <ol className="flex w-full items-center justify-between gap-1 overflow-x-auto">
        {steps.map((s, i) => {
          const isLast = i === steps.length - 1;
          const isActive = s.state === 'active';
          const isDone = s.state === 'done';
          const isFailed = s.state === 'failed';

          return (
            <li key={s.name} className="relative flex flex-1 items-center min-w-[72px] sm:min-w-[80px] lg:min-w-[90px]">
              <button
                type="button"
                onClick={() => onStepClick?.(s.name)}
                className={cn(
                  'group relative z-10 flex flex-col items-center gap-1.5 rounded-lg px-1.5 py-1.5 transition-all',
                  'hover:bg-surface-3/50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-chief',
                  isActive && 'scale-105',
                )}
                aria-label={`${s.label} (${s.state})`}
              >
                {/* Stage Badge / Node Indicator */}
                <span
                  className={cn(
                    'relative flex h-7 w-7 items-center justify-center rounded-full text-xs font-semibold tabular-nums transition-all',
                    isActive &&
                      'border border-chief/80 bg-chief/20 text-white shadow-lg shadow-chief/30 ring-2 ring-chief/40',
                    isDone &&
                      'border border-status-done/60 bg-status-done/15 text-status-done shadow-sm shadow-status-done/20',
                    isFailed &&
                      'border border-status-failed bg-status-failed/20 text-status-failed shadow-sm shadow-status-failed/20',
                    s.state === 'pending' &&
                      'border border-white/5 bg-surface-2 text-text-tertiary group-hover:border-white/10 group-hover:text-text-secondary',
                  )}
                >
                  {isDone ? (
                    <svg className="h-3.5 w-3.5 stroke-[2.5]" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                      <path strokeLinecap="round" strokeLinejoin="round" d="M4.5 12.75l6 6 9-13.5" />
                    </svg>
                  ) : isFailed ? (
                    <span className="text-[11px]">✕</span>
                  ) : (
                    i + 1
                  )}
                </span>

                {/* Step Label */}
                <span
                  className={cn(
                    'truncate text-center text-xs tracking-tight transition-colors',
                    isActive && 'font-medium text-white',
                    isDone && 'font-normal text-text-primary',
                    s.state === 'pending' && 'text-text-tertiary group-hover:text-text-secondary',
                    isFailed && 'font-medium text-status-failed',
                  )}
                >
                  {s.label}
                </span>

                {/* Duration */}
                {s.durationMs !== undefined && (
                  <span className="text-[10px] text-text-tertiary tabular-nums">
                    {s.durationLabel ?? `${Math.round(s.durationMs / 1000)}秒`}
                  </span>
                )}
              </button>

              {/* Right connector track to next step */}
              {!isLast && (
                <div
                  className={cn(
                    'h-[2px] flex-1 mx-1 rounded-full transition-all duration-300',
                    isDone
                      ? 'bg-status-done/70 shadow-[0_0_6px_rgba(16,185,129,0.3)]'
                      : isActive
                        ? 'bg-gradient-to-r from-chief/80 to-surface-3/60'
                        : 'bg-surface-3/60',
                  )}
                  aria-hidden="true"
                />
              )}
            </li>
          );
        })}
      </ol>
    </nav>
  );
}
