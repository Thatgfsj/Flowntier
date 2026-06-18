import type { ReactNode } from 'react';
import { Card, ReasoningBubble, ReviewVerdict } from '@aco/ui';

export interface CenterPanelProps {
  chiefCard: ReactNode;
}

/**
 * Z3 — center panel. Current reasoning / review / task.
 */
export function CenterPanel({ chiefCard }: CenterPanelProps) {
  return (
    <div className="flex flex-col gap-3">
      {chiefCard}

      <ReasoningBubble
        agentName="首席代理"
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
