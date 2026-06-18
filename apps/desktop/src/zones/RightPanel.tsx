import { Card, TaskItem, type TaskState } from '@aco/ui';

export interface RightPanelTask {
  id: string;
  title: string;
  owner: string;
  fileHint?: string;
  state: string;
}

export interface RightPanelProps {
  tasks: ReadonlyArray<RightPanelTask>;
}

const STATE_LABEL: Record<string, string> = {
  PENDING: '待办',
  DISPATCHED: '已派发',
  IN_PROGRESS: '进行中',
  SUBMITTED: '已提交',
  UNDER_REVIEW: '评审中',
  REPAIR_REQUESTED: '需修复',
  REPAIRING: '修复中',
  APPROVED: '已通过',
  DONE: '完成',
  FAILED: '失败',
  ABORTED: '已中止',
};

function toTaskState(s: string): TaskState {
  // The TaskItem type already includes all SimTaskState variants.
  return s as TaskState;
}

/**
 * Z4 — right panel. Task list, progress, current file.
 */
export function RightPanel({ tasks }: RightPanelProps) {
  const total = tasks.length;
  const done = tasks.filter((t) => t.state === 'DONE').length;
  const pct = total > 0 ? Math.round((done / total) * 100) : 0;
  return (
    <div className="flex flex-col gap-3">
      <Card>
        <div className="mb-2 flex items-center justify-between">
          <h2 className="text-sm font-semibold">任务列表</h2>
          <span className="text-xs text-text-secondary">
            {done} / {total} 完成
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
              <TaskItem
                title={t.title}
                state={toTaskState(t.state)}
                owner={t.owner}
                fileHint={t.fileHint}
              />
              <div className="px-1 text-[10px] uppercase tracking-wide text-text-secondary">
                {STATE_LABEL[t.state] ?? t.state}
              </div>
            </div>
          ))}
        </div>
      </Card>
    </div>
  );
}
