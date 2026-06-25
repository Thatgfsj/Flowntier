import { useEffect, useRef, useState } from 'react';
import { PhaseTimeline, AgentCard, Card, type PhaseState, type AgentStatus } from '@flowntier/ui';
import type { WfEvent } from '@flowntier/shared';
import { TopBar } from './zones/TopBar.js';
import { LeftRoster } from './zones/LeftRoster.js';
import { RightPanel } from './zones/RightPanel.js';
import { BottomConsole } from './zones/BottomConsole.js';
import { CommandDock } from './zones/CommandDock.js';
import { Settings } from './zones/Settings.js';
import { PluginsPanel } from './zones/PluginsPanel.js';
import { ReasoningBubble } from '@flowntier/ui';
import { ReviewVerdict } from '@flowntier/ui';
import { PlanGraph, type PlanTaskNode, type PlanEdge } from './components/PlanGraph.js';
import { useEventStream } from './hooks/useEventStream.js';
import { invoke } from '@tauri-apps/api/core';
import { ChatZone } from './zones/ChatZone.js';

interface Phase {
  name:
    | 'requirement'
    | 'planning'
    | 'plan_review'
    | 'dispatch'
    | 'development'
    | 'review'
    | 'repair'
    | 'delivery';
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

const PHASE_STATE: Record<Phase['name'], PhaseState> = {
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
  state: string;
  summary?: string;
}

const INITIAL_TASKS: ReadonlyArray<TaskRow> = [];

type AgentStatusMap = {
  chief: AgentStatus;
  'critic-a': AgentStatus;
  'critic-b': AgentStatus;
  worker: AgentStatus;
};

const INITIAL_AGENT_STATUS: AgentStatusMap = {
  chief: 'idle',
  'critic-a': 'idle',
  'critic-b': 'idle',
  worker: 'idle',
};

function agentStatusToRole(s: AgentStatus): AgentStatus {
  return s;
}

export function App() {
  const [activePhase, setActivePhase] = useState(0);
  const [phaseStates, setPhaseStates] = useState<Record<Phase['name'], PhaseState>>({ ...PHASE_STATE });
  const [tasks, setTasks] = useState<TaskRow[]>([...INITIAL_TASKS]);
  const [agentStatus, setAgentStatus] = useState<AgentStatusMap>({ ...INITIAL_AGENT_STATUS });
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
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [currentWfId, setCurrentWfId] = useState<string | null>(null);
  const [planNodes, setPlanNodes] = useState<PlanTaskNode[]>([]);
  const [planEdges, setPlanEdges] = useState<PlanEdge[]>([]);
  const [selectedTask, setSelectedTask] = useState<string | null>(null);
  const [showPlanGraph, setShowPlanGraph] = useState(false);
  const [chatOpen, setChatOpen] = useState(false);
  const busyRef = useRef(false);

  // Subscribe to the runtime's WfEvent stream. v0.2.5+ delivers events
  // over the `\\.\pipe\flowntier_runtime_events` named pipe → Rust → Tauri
  // `wf:event` broadcast. No more raw WebSocket from the webview.
  const runtimeEvents = useEventStream();
  useEffect(() => {
    if (runtimeEvents.length === 0) return;
    const latest = runtimeEvents[runtimeEvents.length - 1];
    if (latest) applyEvent(latest as WfEvent);
  }, [runtimeEvents]);

  // Expose for screenshot / debug scripts.
  useEffect(() => {
    // @ts-expect-error: window.__flowntierCurrentWfId is a debug hook
    window.__flowntierCurrentWfId = currentWfId;
  }, [currentWfId]);

  // Whenever a new workflow starts, poll /plan until it's ready
  useEffect(() => {
    if (!currentWfId) return;
    let cancelled = false;
    const poll = async () => {
      try {
        // Use Tauri invoke to get workflow plan
        const data = await invoke<Record<string, unknown>>('get_workflow', { id: currentWfId });
        if (cancelled || !data) return;
        // The plan data comes from the workflow state
        // For now, we rely on WebSocket events to populate tasks
      } catch {
        // API call failed
      }
    };
    void poll();
    return () => { cancelled = true; };
  }, [currentWfId]);

  const reset = () => {
    setActivePhase(0);
    setPhaseStates({ ...PHASE_STATE });
    setTasks([]);
    setAgentStatus({ ...INITIAL_AGENT_STATUS });
    setEvents([]);
    setCompleted(false);
    setMilestones([]);
    setReviewVerdict(null);
    setFinalReport(null);
    setCurrentWfId(null);
    setPlanNodes([]);
    setPlanEdges([]);
    setShowPlanGraph(false);
  };

  const applyEvent = (event: WfEvent) => {
    setEvents((prev) => [...prev, event]);
    if (event.kind === 'transition' && event.to) {
      const idx = PHASES.findIndex((p) => p.name === event.to);
      if (idx >= 0) {
        setActivePhase(idx);
        setPhaseStates((prev) => {
          const next = { ...prev };
          for (let i = 0; i < idx; i++) {
            const phaseName = PHASES[i]?.name;
            if (phaseName) next[phaseName] = 'done';
          }
          const toName = event.to as Phase['name'];
          next[toName] = 'active';
          return next;
        });
      }
    }
    if (event.kind === 'milestone' && event.label) {
      setMilestones((prev) => [...prev, event.label]);
    }
    if (event.kind === 'console' && event.agent_id) {
      if (event.agent_id === 'agent:chief') {
        setAgentStatus((prev) => ({ ...prev, chief: 'thinking' }));
      } else if (event.agent_id === 'agent:critic:a') {
        setAgentStatus((prev) => ({ ...prev, 'critic-a': 'thinking' }));
      } else if (event.agent_id === 'agent:critic:b') {
        setAgentStatus((prev) => ({ ...prev, 'critic-b': 'thinking' }));
      } else if (event.agent_id.startsWith('agent:worker:')) {
        setAgentStatus((prev) => ({ ...prev, worker: 'thinking' }));
      }
    }
    if (event.kind === 'task_status' && event.task_id) {
      const newState = event.task_status;
      const summary = event.task_summary;
      setTasks((prev) => {
        const idx = prev.findIndex((t) => t.id === event.task_id);
        if (idx >= 0) {
          const next = prev.slice();
          const existing = next[idx];
          if (existing) {
            const updated: TaskRow = summary
              ? { ...existing, state: newState, summary }
              : { ...existing, state: newState };
            next[idx] = updated;
          }
          return next;
        }
        const fresh: TaskRow = summary
          ? { id: event.task_id!, title: event.task_title ?? event.task_id!, owner: '', state: newState, summary }
          : { id: event.task_id!, title: event.task_title ?? event.task_id!, owner: '', state: newState };
        return [...prev, fresh];
      });
    }
  };

  const startRealWorkflow = async (text: string) => {
    busyRef.current = true;
    setBusy(true);
    setEvents([]);
    setTasks([]);
    setMilestones([]);
    setReviewVerdict(null);
    setFinalReport(null);
    setActivePhase(0);
    setPhaseStates({ ...PHASE_STATE });
    setAgentStatus({ ...INITIAL_AGENT_STATUS });

    // Events arrive via the useEventStream() hook above (Tauri
    // `wf:event` events forwarded from the events named pipe).
    try {
      // Use Tauri invoke to start workflow
      const data = await invoke<{ id: string }>('start_workflow_cmd', { text });
      setCurrentWfId(data.id);

      // Poll for completion using invoke
      const deadline = Date.now() + 600000;
      while (Date.now() < deadline) {
        await new Promise(r => setTimeout(r, 2000));
        try {
          const wf = await invoke<Record<string, unknown>>('get_workflow', { id: data.id });
          if (wf && wf.summary) {
            setFinalReport(wf.summary as string);
            setReviewVerdict({ verdict: 'PASS', summary: '工作流已完成' });
            break;
          }
        } catch {
          // continue polling
        }
      }

      if (!completed) {
        setReviewVerdict({ verdict: 'REPAIR', summary: '工作流超时' });
      }
    } catch (e) {
      console.warn('workflow failed', e);
      setReviewVerdict({ verdict: 'REPAIR', summary: `工作流启动失败: ${e}` });
    } finally {
      busyRef.current = false;
      setBusy(false);
      setCompleted(true);
    }
  };

  const handleSubmit = () => {
    if (completed) {
      reset();
      return;
    }
    const text = cmd.trim() || '实现 POST /auth/login 接口';
    setCmd('');
    void startRealWorkflow(text);
  };

  return (
    <div className="flex h-screen flex-col">
      <TopBar
        projectName="Flowntier"
        subtitle={
          completed
            ? '上次工作流已完成'
            : busy
              ? '运行中…'
              : '准备就绪'
        }
        onSettingsClick={() => setSettingsOpen(true)}
        onChatClick={() => setChatOpen((v) => !v)}
        chatOpen={chatOpen}
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

          {showPlanGraph && planNodes.length > 0 && (
            <Card className="mb-3">
              <div className="mb-2 flex items-center justify-between">
                <h3 className="text-sm font-semibold">计划图</h3>
                <span className="text-xs text-text-secondary">
                  {planNodes.length} 个任务
                </span>
              </div>
              <PlanGraph
                nodes={planNodes}
                edges={planEdges}
                onNodeClick={setSelectedTask}
                className="h-[350px] w-full rounded border border-border"
              />
              {selectedTask && (
                <div className="mt-2 rounded bg-surface-2 p-2 text-xs">
                  <span className="font-semibold">选中：</span>
                  {planNodes.find((n) => n.id === selectedTask)?.title ?? selectedTask}
                </div>
              )}
            </Card>
          )}

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

          <AgentCard
            role="chief"
            name="主理"
            status={agentStatusToRole(agentStatus.chief)}
            subtitle={
              agentStatus.chief === 'thinking'
                ? '沉稳的策略师 · 正在分析'
                : agentStatus.chief === 'speaking'
                  ? '沉稳的策略师 · 正在汇报'
                  : '沉稳的策略师 · 待命'
            }
            progress={busy ? 0.5 : undefined}
          />

          <ReasoningBubble
            agentName="主理"
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
          <RightPanel tasks={tasks} events={events} />
          <div className="mt-3">
            <PluginsPanel />
          </div>
        </aside>
      </div>

      <CommandDock
        commandInput={cmd}
        onCommandChange={setCmd}
        onCommandSubmit={handleSubmit}
        busy={busy}
        {...(completed ? { resetLabel: '重置' } : {})}
      />

      {/* v0.3 ChatZone — progressive. Collapsed by default; toggle via TopBar. */}
      <div
        className={`relative flex shrink-0 border-t border-border transition-[height] ${
          chatOpen ? 'h-[420px]' : 'h-9'
        }`}
      >
        {chatOpen ? (
          <>
            <div className="h-full w-full">
              <ChatZone />
            </div>
            <button
              type="button"
              onClick={() => setChatOpen(false)}
              className="absolute right-2 top-1 rounded border border-border bg-surface-2 px-2 py-0.5 text-xs text-text-secondary hover:bg-surface-1"
              aria-label="折叠 ChatZone"
            >
              ▾ 折叠
            </button>
          </>
        ) : (
          <button
            type="button"
            onClick={() => setChatOpen(true)}
            className="flex h-9 w-full items-center justify-between gap-2 bg-surface-2 px-4 text-left text-xs text-text-secondary hover:bg-surface-1"
            aria-label="打开 ChatZone"
          >
            <span className="font-mono">ChatZone ▸</span>
            <span>直接驱动 agent-core · 点这里展开</span>
          </button>
        )}
      </div>

      <BottomConsole events={events} />

      <Settings open={settingsOpen} onClose={() => setSettingsOpen(false)} />
    </div>
  );
}
