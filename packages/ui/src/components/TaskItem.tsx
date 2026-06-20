import { cn } from '../lib/cn.js';

export type TaskState =
  | 'PENDING'
  | 'DISPATCHED'
  | 'IN_PROGRESS'
  | 'SUBMITTED'
  | 'UNDER_REVIEW'
  | 'APPROVED'
  | 'REPAIR_REQUESTED'
  | 'REPAIRING'
  | 'REJECTED'
  | 'DONE'
  | 'FAILED'
  | 'ABORTED';

export interface TaskItemProps {
  title: string;
  state: TaskState;
  owner?: string;
  durationMs?: number;
  fileHint?: string | undefined;
  className?: string;
}

const stateIcon: Record<TaskState, string> = {
  PENDING: '○',
  DISPATCHED: '◐',
  IN_PROGRESS: '◑',
  SUBMITTED: '◓',
  UNDER_REVIEW: '◔',
  APPROVED: '✓',
  REPAIR_REQUESTED: '⚠',
  REPAIRING: '↻',
  REJECTED: '✗',
  DONE: '✓',
  FAILED: '✗',
  ABORTED: '⊘',
};

const stateColor: Record<TaskState, string> = {
  PENDING: 'text-status-pending',
  DISPATCHED: 'text-status-active',
  IN_PROGRESS: 'text-status-active',
  SUBMITTED: 'text-status-active',
  UNDER_REVIEW: 'text-status-warn',
  APPROVED: 'text-status-done',
  REPAIR_REQUESTED: 'text-status-warn',
  REPAIRING: 'text-status-warn',
  REJECTED: 'text-status-failed',
  DONE: 'text-status-done',
  FAILED: 'text-status-failed',
  ABORTED: 'text-status-pending',
};

export function TaskItem({
  title,
  state,
  owner,
  durationMs,
  fileHint,
  className,
}: TaskItemProps) {
  return (
    <div
      className={cn(
        'flex items-center gap-3 rounded-md border border-border bg-surface-1 p-2 text-sm',
        className,
      )}
    >
      <span className={cn('w-5 text-center text-base', stateColor[state])} aria-hidden="true">
        {stateIcon[state]}
      </span>
      <div className="flex-1 min-w-0">
        <div className="truncate font-medium">{title}</div>
        {(owner || fileHint) && (
          <div className="truncate text-xs text-text-secondary">
            {owner && <span>{owner}</span>}
            {owner && fileHint && <span> · </span>}
            {fileHint && <span className="font-mono">{fileHint}</span>}
          </div>
        )}
      </div>
      {durationMs !== undefined && (
        <span className="shrink-0 text-xs text-text-secondary tabular-nums">
          {Math.round(durationMs / 1000)}s
        </span>
      )}
    </div>
  );
}
