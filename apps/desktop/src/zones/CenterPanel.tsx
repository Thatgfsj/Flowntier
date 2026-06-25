import type { ReactNode } from 'react';
import { Card, ReasoningBubble, ReviewVerdict } from '@flowntier/ui';

export interface CenterPanelProps {
  chiefCard: ReactNode;
  /**
   * True when a workflow is currently running OR just finished.
   * When false, we render an empty-state guidance card instead
   * of the demo reasoning content.
   */
  hasActiveWorkflow: boolean;
  /**
   * Optional callback for the empty-state "Try sample" button.
   * If absent, the button is hidden (e.g. during loading).
   */
  onTrySample?: () => void;
}

/**
 * Z3 — center panel. Current reasoning / review / task.
 *
 * Two modes:
 *   hasActiveWorkflow=true  : show live chief + reviewer output.
 *   hasActiveWorkflow=false : show an empty-state guidance card
 *                              ("no workflow yet, here's how to
 *                              start one") with a "Try sample"
 *                              shortcut.
 */
export function CenterPanel({ chiefCard, hasActiveWorkflow, onTrySample }: CenterPanelProps) {
  if (!hasActiveWorkflow) {
    return (
      <div className="flex flex-col gap-3">
        <Card>
          <div className="flex flex-col items-start gap-3 py-6 text-center">
            <div className="self-center text-3xl">▶</div>
            <h3 className="text-base font-semibold text-text-primary">还没有工作流</h3>
            <p className="text-sm text-text-secondary">
              在下方命令栏键入一个任务，比如:
            </p>
            <ul className="self-start space-y-1 text-left text-sm text-text-secondary">
              <li>• <span className="font-mono text-xs">给项目加单元测试</span></li>
              <li>• <span className="font-mono text-xs">实现 POST /auth/login 接口</span></li>
              <li>• <span className="font-mono text-xs">重构 src/components/Sidebar.tsx</span></li>
            </ul>
            {onTrySample && (
              <button
                type="button"
                onClick={onTrySample}
                className="mt-2 self-center rounded-md bg-accent px-4 py-2 text-sm font-medium text-white transition-opacity hover:opacity-90"
              >
                或试试示例任务 →
              </button>
            )}
          </div>
        </Card>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-3">
      {chiefCard}

      <ReasoningBubble
        agentName="主理"
        roleColorClass="border-t-chief"
        step="规划中 — 草拟 API 设计"
        body="正在草拟 4 个任务的计划：后端 /login 接口、前端 LoginForm、数据库 users 表、单元测试。预计输入 9k tokens，输出 4k。"
        ago="2 秒前"
      />

      <Card>
        <h3 className="mb-2 text-sm font-semibold">审核员 B — 架构审查</h3>
        <ReviewVerdict
          verdict="PASS"
          confidence={0.87}
          issues={[]}
          summary="模块边界清晰，鉴权模块与路由处理器解耦，结构良好。"
        />
      </Card>
    </div>
  );
}
