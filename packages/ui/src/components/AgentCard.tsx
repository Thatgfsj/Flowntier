import type { ReactNode } from 'react';
import { cn } from '../lib/cn.js';

export type AgentRole = 'chief' | 'critic-a' | 'critic-b' | 'worker';
export type AgentStatus = 'idle' | 'thinking' | 'speaking' | 'error';

export interface AgentCardProps {
  role: AgentRole;
  name: string;
  status: AgentStatus;
  /** Optional sub-line, e.g. "Calm strategist". */
  subtitle?: string;
  /** Optional localized label for the status pill. Falls back
   *  to the raw enum if not provided. */
  statusLabel?: string;
  /** Optional avatar element (e.g. Live2D in v0.5). */
  avatar?: ReactNode;
  /** Optional progress 0..1. */
  progress?: number | undefined;
  className?: string;
}

const roleAccents: Record<AgentRole, string> = {
  chief: 'from-chief to-chief/30 text-chief',
  'critic-a': 'from-critic-a to-critic-a/30 text-critic-a',
  'critic-b': 'from-critic-b to-critic-b/30 text-critic-b',
  worker: 'from-worker-1 to-worker-1/30 text-worker-1',
};

const statusPills: Record<AgentStatus, Record<AgentRole, string>> = {
  idle: {
    chief: 'bg-surface-3/80 text-text-secondary border border-white/5',
    'critic-a': 'bg-surface-3/80 text-text-secondary border border-white/5',
    'critic-b': 'bg-surface-3/80 text-text-secondary border border-white/5',
    worker: 'bg-surface-3/80 text-text-secondary border border-white/5',
  },
  thinking: {
    chief: 'bg-chief/15 text-chief border border-chief/30 shadow-sm shadow-chief/20 animate-pulse',
    'critic-a': 'bg-critic-a/15 text-critic-a border border-critic-a/30 shadow-sm shadow-critic-a/20 animate-pulse',
    'critic-b': 'bg-critic-b/15 text-critic-b border border-critic-b/30 shadow-sm shadow-critic-b/20 animate-pulse',
    worker: 'bg-worker-1/15 text-worker-1 border border-worker-1/30 shadow-sm shadow-worker-1/20 animate-pulse',
  },
  speaking: {
    chief: 'bg-chief/20 text-white border border-chief/40 shadow-sm shadow-chief/20',
    'critic-a': 'bg-critic-a/20 text-white border border-critic-a/40 shadow-sm shadow-critic-a/20',
    'critic-b': 'bg-critic-b/20 text-white border border-critic-b/40 shadow-sm shadow-critic-b/20',
    worker: 'bg-worker-1/20 text-white border border-worker-1/40 shadow-sm shadow-worker-1/20',
  },
  error: {
    chief: 'bg-status-failed/20 text-status-failed border border-status-failed/40',
    'critic-a': 'bg-status-failed/20 text-status-failed border border-status-failed/40',
    'critic-b': 'bg-status-failed/20 text-status-failed border border-status-failed/40',
    worker: 'bg-status-failed/20 text-status-failed border border-status-failed/40',
  },
};

const DEFAULT_STATUS_LABELS: Record<AgentStatus, string> = {
  idle: 'idle',
  thinking: 'thinking',
  speaking: 'speaking',
  error: 'error',
};

/**
 * Modern Card representing a single agent.
 */
export function AgentCard({
  role,
  name,
  status,
  subtitle,
  statusLabel,
  avatar,
  progress,
  className,
}: AgentCardProps) {
  return (
    <div
      className={cn(
        'relative flex items-center gap-3 overflow-hidden rounded-xl border border-white/8 bg-surface-2/90 p-3 shadow-sm backdrop-blur-md transition-all hover:border-white/15 hover:shadow-md',
        className,
      )}
      role="img"
      aria-label={`${name}, ${status}`}
    >
      {/* Sleek left gradient accent bar */}
      <div
        className={cn('absolute inset-y-0 left-0 w-1 bg-gradient-to-b', roleAccents[role])}
        aria-hidden="true"
      />

      <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg border border-white/5 bg-surface-3/90 text-xs font-semibold tracking-wider text-text-primary shadow-inner">
        {avatar ?? name.slice(0, 2).toUpperCase()}
      </div>

      <div className="flex-1 min-w-0">
        <div className="flex items-center justify-between gap-1.5">
          <div className="truncate text-xs font-semibold tracking-tight text-text-primary">{name}</div>
          <span
            className={cn(
              'inline-flex shrink-0 items-center gap-1 rounded-full px-2 py-0.5 text-[10px] font-medium tracking-wide transition-all',
              statusPills[status][role],
            )}
          >
            {status === 'thinking' && (
              <span className="relative flex h-2 w-2 items-center justify-center">
                <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-current opacity-75" />
                <span className="relative inline-flex h-1.5 w-1.5 rounded-full bg-current" />
              </span>
            )}
            {status === 'speaking' && (
              <span className="h-1.5 w-1.5 rounded-full bg-current animate-pulse" />
            )}
            {statusLabel ?? DEFAULT_STATUS_LABELS[status]}
          </span>
        </div>

        {subtitle && (
          <div className="truncate text-[11px] text-text-secondary mt-0.5">{subtitle}</div>
        )}

        {progress !== undefined && (
          <div
            className="mt-2 h-1 w-full rounded-full bg-surface-3 overflow-hidden"
            aria-hidden="true"
          >
            <div
              className="h-full bg-gradient-to-r from-chief to-status-done transition-all duration-300"
              style={{ width: `${Math.min(100, Math.max(0, progress * 100))}%` }}
            />
          </div>
        )}
      </div>
    </div>
  );
}
