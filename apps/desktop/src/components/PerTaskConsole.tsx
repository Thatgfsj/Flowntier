/**
 * PerTaskConsole — per-task console output viewer.
 *
 * Shows console logs filtered by task ID with agent attribution.
 * See `docs/UI_GUIDELINES.md` §6.6.
 */

import { useMemo, useState } from 'react';
import type { WfEvent, LogLevel } from '@aco/shared';

export interface PerTaskConsoleProps {
  taskId: string;
  events: readonly WfEvent[];
  className?: string;
}

const LEVEL_COLORS: Record<LogLevel, string> = {
  error: 'text-status-failed',
  warn: 'text-status-warn',
  info: 'text-primary',
  debug: 'text-text-secondary',
  trace: 'text-text-secondary',
};

const LEVEL_LABELS: Record<LogLevel, string> = {
  error: 'ERR',
  warn: 'WRN',
  info: 'INF',
  debug: 'DBG',
  trace: 'TRC',
};

function shortTime(iso: string): string {
  return iso.slice(11, 19);
}

function agentToLabel(agentId: string): string {
  if (agentId === 'agent:chief') return '首席';
  if (agentId === 'agent:critic:a') return '审核A';
  if (agentId === 'agent:critic:b') return '审核B';
  if (agentId.startsWith('agent:worker:')) return '执行员';
  if (agentId === 'agent:system') return '系统';
  return agentId;
}

export function PerTaskConsole({ taskId, events, className }: PerTaskConsoleProps) {
  const [filter, setFilter] = useState<LogLevel | 'all'>('all');

  // Filter events for this task
  const taskEvents = useMemo(() => {
    return events.filter((e) => {
      // Match task_id in task_status events
      if (e.kind === 'task_status' && e.task_id === taskId) return true;
      // Match console events that reference this task
      if (e.kind === 'console' && e.message.includes(taskId)) return true;
      return false;
    });
  }, [events, taskId]);

  // Apply level filter
  const filtered = useMemo(() => {
    if (filter === 'all') return taskEvents;
    return taskEvents.filter((e) => {
      if (e.kind === 'console') return e.level === filter;
      return true; // task_status events always show
    });
  }, [taskEvents, filter]);

  if (taskEvents.length === 0) {
    return (
      <div className={`text-xs text-text-secondary ${className ?? ''}`}>
        暂无此任务的日志
      </div>
    );
  }

  return (
    <div className={className}>
      {/* Level filter */}
      <div className="mb-2 flex items-center gap-1">
        <span className="text-[10px] text-text-secondary">过滤:</span>
        {(['all', 'error', 'warn', 'info', 'debug'] as const).map((level) => (
          <button
            key={level}
            onClick={() => setFilter(level)}
            className={`rounded px-1.5 py-0.5 text-[10px] transition-colors ${
              filter === level
                ? 'bg-status-active text-white'
                : 'bg-surface-3 text-text-secondary hover:bg-surface-2'
            }`}
          >
            {level === 'all' ? '全部' : LEVEL_LABELS[level]}
          </button>
        ))}
      </div>

      {/* Log lines */}
      <div className="max-h-48 overflow-y-auto font-mono text-[11px]">
        {filtered.map((e, i) => {
          if (e.kind === 'task_status') {
            return (
              <div key={i} className="flex gap-2 py-0.5">
                <span className="shrink-0 text-text-secondary">
                  {e.ts ? shortTime(e.ts) : '--:--:--'}
                </span>
                <span className="shrink-0 text-status-active">任务</span>
                <span className="text-primary">
                  状态变更: {e.task_status}
                  {e.task_summary ? ` — ${e.task_summary}` : ''}
                </span>
              </div>
            );
          }

          if (e.kind === 'console') {
            return (
              <div key={i} className="flex gap-2 py-0.5">
                <span className="shrink-0 text-text-secondary">
                  {shortTime(new Date().toISOString())}
                </span>
                <span className={`shrink-0 ${LEVEL_COLORS[e.level]}`}>
                  {LEVEL_LABELS[e.level]}
                </span>
                <span className="shrink-0 text-text-secondary">
                  {agentToLabel(e.agent_id)}
                </span>
                <span className="min-w-0 flex-1 truncate text-primary">
                  {e.message}
                </span>
              </div>
            );
          }

          return null;
        })}
      </div>

      {/* Stats */}
      <div className="mt-2 text-[10px] text-text-secondary">
        共 {taskEvents.length} 条记录
      </div>
    </div>
  );
}
