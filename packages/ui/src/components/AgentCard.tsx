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

const roleColors: Record<AgentRole, string> = {
  chief: 'border-l-chief',
  'critic-a': 'border-l-critic-a',
  'critic-b': 'border-l-critic-b',
  worker: 'border-l-worker-1',
};

const statusPills: Record<AgentStatus, string> = {
  idle: 'bg-status-pending/20 text-status-pending',
  thinking: 'bg-status-active/20 text-status-active animate-pulse',
  speaking: 'bg-status-active/20 text-status-active',
  error: 'bg-status-failed/20 text-status-failed',
};

/** BUG-FRONTEND-RT-5 (event 000031): raw English status
 *  (`idle` / `thinking` / `speaking` / `error`) was rendered
 *  verbatim on the agent card pill — users saw "IDLE" instead
 *  of "Idle" / "思考中". Now the consumer can pass an optional
 *  `statusLabel` prop (typically via `t('agentCard.status.<id>')`)
 *  to localize. Falls back to the raw enum when not provided. */
const DEFAULT_STATUS_LABELS: Record<AgentStatus, string> = {
  idle: 'idle',
  thinking: 'thinking',
  speaking: 'speaking',
  error: 'error',
};

/**
 * Card representing a single agent. See `docs/UI_GUIDELINES.md` §6.1.
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
        'flex items-center gap-3 rounded-md border border-border border-l-4 bg-surface-1 p-3',
        roleColors[role],
        className,
      )}
      role="img"
      aria-label={`${name}, ${status}`}
    >
      <div className="h-10 w-10 shrink-0 rounded-full bg-surface-3 flex items-center justify-center text-xs">
        {avatar ?? name.slice(0, 2).toUpperCase()}
      </div>
      <div className="flex-1 min-w-0">
        <div className="flex items-center justify-between gap-2">
          <div className="font-medium truncate">{name}</div>
          <span
            className={cn(
              'shrink-0 rounded-full px-2 py-0.5 text-[10px] uppercase tracking-wide',
              statusPills[status],
            )}
          >
            {statusLabel ?? DEFAULT_STATUS_LABELS[status]}
          </span>
        </div>
        {subtitle && (
          <div className="text-xs text-text-secondary truncate">{subtitle}</div>
        )}
        {progress !== undefined && (
          <div
            className="mt-1.5 h-1 w-full rounded-full bg-surface-3 overflow-hidden"
            aria-hidden="true"
          >
            <div
              className="h-full bg-chief transition-all"
              style={{ width: `${Math.min(100, Math.max(0, progress * 100))}%` }}
            />
          </div>
        )}
      </div>
    </div>
  );
}
