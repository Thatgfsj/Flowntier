import { useEffect, useRef, useState } from 'react';
import { PhaseTimeline, AgentCard, Card, type PhaseState, type AgentStatus } from '@aco/ui';
import type { WfEvent } from '@aco/shared';
import { TopBar } from './zones/TopBar.js';
import { LeftRoster } from './zones/LeftRoster.js';
import { CenterPanel } from './zones/CenterPanel.js';
import { RightPanel } from './zones/RightPanel.js';
import { BottomConsole } from './zones/BottomConsole.js';
import { CommandDock } from './zones/CommandDock.js';
import { ReasoningBubble } from '@aco/ui';
import { ReviewVerdict } from '@aco/ui';
import {
  startSimulation,
  type SimEvent,
  type SimPhaseId,
  type SimTaskState,
  type SimAgentStatus,
} from './simulator.js';

interface Phase {
  name: SimPhaseId;
  label: string;
}

const PHASES: ReadonlyArray<Phase> = [
  { name: 'requirement', label: '需求' },
  { name: 'planning', label: '规划' },
  { name: 'plan_review', label: '计划审核' },
  { name: 'dispatch', label: '派发' },
  { name: 'development', label: '开发' },
  { name: 'review', label: '评审' },
  { name: 'repair', label: '修复' },
  { name: 'delivery', label: '交付' },
];

const PHASE_STATE: Record<SimPhaseId, PhaseState> = {
  requirement: 'pending',
  planning: 'pending',
  plan_review: 'pending',
  dispatch: 'pending',
  development: 'pending',
  review: 'pending',
  repair: 'pending',
  delivery: 'pending',
};

interface TaskRow {
  id: string;
  title: string;
  owner: string;
  fileHint?: string;
  state: SimTaskState;
}

const INITIAL_TASKS: ReadonlyArray<TaskRow> = [
  { id: 't1', title: '后端：实现 /login 接口', owner: '执行员 1', fileHint: 'src/auth/login.py', state: 'PENDING' },
  { id: 't2', title: '前端：LoginForm 组件', owner: '执行员 2', fileHint: 'src/components/LoginForm.tsx', state: 'PENDING' },
  { id: 't3', title: '数据库：users 表迁移', owner: '执行员 3', fileHint: 'migrations/0001_users.sql', state: 'PENDING' },
  { id: 't4', title: '测试：登录流程端到端', owner: '执行员 4', fileHint: 'tests/e2e/test_login.py', state: 'PENDING' },
];

const INITIAL_AGENT_STATUS: SimAgentStatus = {
  chief: 'idle',
  'critic-a': 'idle',
  'critic-b': 'idle',
  worker: 'idle',
};

function nowIso(): string {
  return new Date().toISOString();
}

function stateToLabel(s: SimTaskState): string {
  return s;
}

function chiefCardFromStatus(status: AgentStatus, progress?: number) {
  return (
    <AgentCard
      role="chief"
      name="首席代理"
      status={status}
      subtitle={
        status === 'thinking'
          ? '沉稳的策略师 · 正在分析'
          : status === 'speaking'
            ? '沉稳的策略师 · 正在汇报'
            : '沉稳的策略师 · 待命'
      }
      progress={progress}
    />
  );
}

function agentStatusToRole(s: SimAgentStatus['chief']): AgentStatus {
  return s;
}

export function App() {
  const [activePhase, setActivePhase] = useState(0);
  const [phaseStates, setPhaseStates] = useState<Record<SimPhaseId, PhaseState>>({ ...PHASE_STATE });
  const [tasks, setTasks] = useState<TaskRow[]>([...INITIAL_TASKS]);
  const [agentStatus, setAgentStatus] = useState<SimAgentStatus>({ ...INITIAL_AGENT_STATUS });
  const [events, setEvents] = useState<WfEvent[]>([]);
  const [cmd, setCmd] = useState('');
  const [busy, setBusy] = useState(false);
  const [completed, setCompleted] = useState(false);
  const [milestones, setMilestones] = useState<string[]>([]);
  const [reviewVerdict, setReviewVerdict] = useState<{
    verdict: 'PASS' | 'REPAIR' | 'REWRITE';
    summary: string;
  } | null>(null);
  const [finalReport, setFinalReport] = useState<string | null>(null);
  const simRef = useRef<(() => void) | null>(null);

  const appendLog = (agent_id: string, level: 'info' | 'warn' | 'error' | 'debug', message: string) => {
    setEvents((prev) => [
      ...prev,
      { kind: 'console', ts: nowIso(), agent_id, level, message },
    ]);
  };

  const reset = () => {
    simRef.current?.();
    simRef.current = null;
    setActivePhase(0);
    setPhaseStates({ ...PHASE_STATE });
    setTasks([...INITIAL_TASKS]);
    setAgentStatus({ ...INITIAL_AGENT_STATUS });
    setEvents([]);
    setCompleted(false);
    setMilestones([]);
    setReviewVerdict(null);
    setFinalReport(null);
  };

  const start = () => {
    if (busy) return;
    setBusy(true);
    appendLog('agent:user', `> ${cmd.trim() || '实现 POST /auth/login 接口'}`, 'info');
    simRef.current = startSimulation(
      {
        onEvent: (e: SimEvent) => {
          if (e.log) appendLog(e.log.agent_id, e.log.level, e.log.message);
          if (e.phase) {
            setPhaseStates((prev) => ({ ...prev, [e.phase!.name]: e.phase!.state }));
            const idx = PHASES.findIndex((p) => p.name === e.phase!.name);
            if (idx >= 0 && e.phase.state === 'active') setActivePhase(idx);
          }
          if (e.task) {
            setTasks((prev) =>
              prev.map((t) => (t.id === e.task!.id ? { ...t, state: e.task!.state } : t)),
            );
          }
          if (e.agent) setAgentStatus((prev) => ({ ...prev, ...e.agent }));
          if (e.milestone) {
            setMilestones((prev) => [...prev, e.milestone!]);
          }
          if (e.done) {
            if (e.done.status === 'DONE') {
              setReviewVerdict({
                verdict: 'PASS',
                summary: '所有任务通过最终评审。',
              });
              setFinalReport(
                '4 个任务全部完成：后端 /login 接口、前端 LoginForm、数据库迁移、端到端测试。\n修改文件：src/auth/login.py, src/components/LoginForm.tsx, migrations/0001_users.sql, tests/e2e/test_login.py\n下一个建议：接入真实 LLM provider（v0.3）。',
              );
            }
          }
        },
        onComplete: () => {
          setBusy(false);
          setCompleted(true);
        },
      },
      { speed: 2.0, triggerRepair: true },
    );
  };

  useEffect(() => {
    return () => simRef.current?.();
  }, []);

  const handleSubmit = () => {
    if (completed) {
      reset();
      return;
    }
    start();
  };

  return (
    <div className="flex h-screen flex-col">
      <TopBar
        projectName="Agent Company OS"
        subtitle={completed ? '上次工作流已完成' : busy ? '运行中…' : '示例工作流：实现登录接口'}
      />

      <div className="flex flex-1 overflow-hidden">
        <aside
          className="w-[260px] shrink-0 overflow-y-auto border-r border-border bg-surface-2 p-2"
          aria-label="智能体名册"
        >
          <LeftRoster
            chiefStatus={agentStatusToRole(agentStatus.chief)}
            criticAStatus={agentStatusToRole(agentStatus['critic-a'])}
            criticBStatus={agentStatusToRole(agentStatus['critic-b'])}
            workerStatus={agentStatusToRole(agentStatus.worker)}
          />
        </aside>

        <main className="flex-1 overflow-y-auto p-3">
          <div className="mb-3 rounded-lg border border-border bg-surface-1 p-2">
            <PhaseTimeline
              steps={PHASES.map((p) => ({
                name: p.name,
                label: p.label,
                state: phaseStates[p.name],
              }))}
              onStepClick={(name) => {
                const idx = PHASES.findIndex((p) => p.name === name);
                if (idx >= 0) setActivePhase(idx);
              }}
            />
          </div>

          {milestones.length > 0 && (
            <Card className="mb-3">
              <h3 className="mb-1 text-xs font-semibold uppercase tracking-wide text-text-secondary">
                里程碑
              </h3>
              <ul className="text-sm text-primary">
                {milestones.slice(-5).map((m, i) => (
                  <li key={i} className="font-mono text-xs">▸ {m}</li>
                ))}
              </ul>
            </Card>
          )}

          {chiefCardFromStatus(agentStatusToRole(agentStatus.chief), 0.5)}

          <ReasoningBubble
            agentName="首席代理"
            roleColorClass="border-t-chief"
            step={`阶段 ${activePhase + 1} / 8`}
            body={
              completed
                ? '工作流已完成。请查看右侧交付摘要。'
                : busy
                  ? '正在协调团队执行用户指令…'
                  : '等待用户在下方的命令栏输入指令。'
            }
            ago={busy ? '刚刚' : '待命中'}
          />

          {reviewVerdict !== null && (
            <Card>
              <h3 className="mb-2 text-sm font-semibold">最终评审</h3>
              <ReviewVerdict
                verdict={reviewVerdict.verdict}
                confidence={1.0}
                issues={[]}
                summary={reviewVerdict.summary}
              />
            </Card>
          )}

          {finalReport !== null && (
            <Card>
              <h3 className="mb-2 text-sm font-semibold">交付摘要</h3>
              <pre className="whitespace-pre-wrap font-mono text-xs text-primary">
                {finalReport}
              </pre>
            </Card>
          )}
        </main>

        <aside
          className="w-[360px] shrink-0 overflow-y-auto border-l border-border bg-surface-2 p-3"
          aria-label="任务面板"
        >
          <RightPanel tasks={tasks} />
        </aside>
      </div>

      <CommandDock
        commandInput={cmd}
        onCommandChange={setCmd}
        onCommandSubmit={handleSubmit}
        busy={busy}
        resetLabel={completed ? '重置' : undefined}
      />

      <BottomConsole events={events} />
    </div>
  );
}
