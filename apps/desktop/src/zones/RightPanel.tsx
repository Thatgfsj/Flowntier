/**
 * Z4 — right panel. Task list, progress, current file, per-task console.
 *
 * See `docs/UI_GUIDELINES.md` §6.4 and §6.6.
 */

import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Card, TaskItem, type TaskState } from '@flowntier/ui';
import type { WfEvent } from '@flowntier/shared';
import { PerTaskConsole } from '../components/PerTaskConsole.js';

export interface RightPanelTask {
  id: string;
  title: string;
  owner: string;
  fileHint?: string;
  state: string;
  summary?: string;
}

export interface RightPanelProps {
  tasks: ReadonlyArray<RightPanelTask>;
  events?: readonly WfEvent[];
}

const STATE_LABEL: Record<string, string> = {
  PENDING: '待办',
  DISPATCHED: '已派发',
  IN_PROGRESS: '进行中',
  SUBMITTED: '已提交',
  UNDER_REVIEW: '评审中',
  REPAIR_REQUESTED: '需修复',
  REPAIRING: '修复中',
  // BUG-FRONTEND-2 (audit 000026 #86/87): REJECTED was
  // missing from both maps, so a task in REJECTED state
  // rendered as "?" + the raw "REJECTED" string.
  REJECTED: '已驳回',
  APPROVED: '已通过',
  DONE: '完成',
  FAILED: '失败',
  ABORTED: '已中止',
};

const STATE_ICONS: Record<string, string> = {
  PENDING: '○',
  DISPATCHED: '◐',
  IN_PROGRESS: '◑',
  SUBMITTED: '◓',
  UNDER_REVIEW: '◔',
  REPAIR_REQUESTED: '⚠',
  REPAIRING: '↻',
  REJECTED: '✗',
  APPROVED: '✓',
  DONE: '✓',
  FAILED: '✗',
  ABORTED: '⊘',
};

function toTaskState(s: string): TaskState {
  return s as TaskState;
}

/**
 * Z4 — right panel. Task list, progress, current file.
 */
export function RightPanel({ tasks, events = [] }: RightPanelProps) {
  const { t } = useTranslation();
  const [selectedTask, setSelectedTask] = useState<string | null>(null);
  const [showConsole, setShowConsole] = useState(false);

  const total = tasks.length;
  const done = tasks.filter((t) => t.state === 'DONE').length;
  const pct = total > 0 ? Math.round((done / total) * 100) : 0;

  const selected = selectedTask ? tasks.find((t) => t.id === selectedTask) : null;

  return (
    <div className="flex flex-col gap-3">
      {/* Task list */}
      <Card>
        <div className="mb-2 flex items-center justify-between">
          <h2 className="text-sm font-semibold">{t('rightPanel.taskList')}</h2>
          <span className="text-xs text-text-secondary">
            {done} / {total} {t('rightPanel.done')}
          </span>
        </div>
        <div className="mb-2 h-1.5 w-full overflow-hidden rounded-full bg-surface-3">
          <div
            className="h-full bg-status-done transition-all"
            style={{ width: `${pct}%` }}
          />
        </div>
        <div className="flex flex-col gap-1.5">
          {tasks.map((t) => (
            <div key={t.id} className="flex flex-col gap-0.5">
              <button
                onClick={() => {
                  setSelectedTask(selectedTask === t.id ? null : t.id);
                  setShowConsole(false);
                }}
                className={`text-left transition-colors ${
                  selectedTask === t.id ? 'rounded bg-surface-2 p-1' : ''
                }`}
              >
                <TaskItem
                  title={t.title}
                  state={toTaskState(t.state)}
                  owner={t.owner}
                  fileHint={t.fileHint}
                />
              </button>
              <div className="flex items-center justify-between px-1">
                <span className="text-[10px] uppercase tracking-wide text-text-secondary">
                  {STATE_ICONS[t.state] ?? '?'} {STATE_LABEL[t.state] ?? t.state}
                </span>
                {t.summary && (
                  <span className="max-w-[200px] truncate text-[10px] text-text-secondary">
                    {t.summary}
                  </span>
                )}
              </div>
            </div>
          ))}
        </div>
      </Card>

      {/* Task detail panel */}
      {selected && (
        <Card>
          <div className="mb-2 flex items-center justify-between">
            <h3 className="text-sm font-semibold">任务详情</h3>
            <button
              onClick={() => setShowConsole(!showConsole)}
              className="text-xs text-status-active hover:underline"
            >
              {showConsole ? t('rightPanel.hideLogs') : t('rightPanel.viewLogs')}
            </button>
          </div>

          <div className="space-y-2 text-xs">
            <div>
              <span className="text-text-secondary">ID: </span>
              <span className="font-mono text-primary">{selected.id}</span>
            </div>
            <div>
              <span className="text-text-secondary">标题: </span>
              <span className="text-primary">{selected.title}</span>
            </div>
            <div>
              <span className="text-text-secondary">负责人: </span>
              <span className="text-primary">{selected.owner || t('rightPanel.unassigned')}</span>
            </div>
            {selected.fileHint && (
              <div>
                <span className="text-text-secondary">文件: </span>
                <span className="font-mono text-primary">{selected.fileHint}</span>
              </div>
            )}
            <div>
              <span className="text-text-secondary">状态: </span>
              <span className="text-primary">
                {STATE_ICONS[selected.state] ?? '?'}{' '}
                {STATE_LABEL[selected.state] ?? selected.state}
              </span>
            </div>
            {selected.summary && (
              <div>
                <span className="text-text-secondary">摘要: </span>
                <span className="text-primary">{selected.summary}</span>
              </div>
            )}
          </div>

          {/* Per-task console */}
          {showConsole && (
            <div className="mt-3 rounded bg-surface-2 p-2">
              <h4 className="mb-1 text-xs font-medium">任务日志</h4>
              <PerTaskConsole taskId={selected.id} events={events} />
            </div>
          )}
        </Card>
      )}

      {/* Task tree summary */}
      {tasks.length > 0 && (
        <Card>
          <h3 className="mb-2 text-sm font-semibold">任务树</h3>
          <div className="space-y-1 font-mono text-xs">
            {tasks.map((t) => (
              <div
                key={t.id}
                className={`flex items-center gap-2 rounded px-1 py-0.5 ${
                  selectedTask === t.id ? 'bg-surface-2' : ''
                }`}
              >
                <span
                  className={
                    t.state === 'DONE'
                      ? 'text-status-done'
                      : t.state === 'FAILED'
                        ? 'text-status-failed'
                        : t.state === 'IN_PROGRESS'
                          ? 'text-status-active'
                          : 'text-text-secondary'
                  }
                >
                  {STATE_ICONS[t.state] ?? '?'}
                </span>
                <span className="truncate text-primary">{t.title}</span>
                <span className="ml-auto text-[10px] text-text-secondary">
                  {t.owner}
                </span>
              </div>
            ))}
          </div>
        </Card>
      )}
    </div>
  );
}
