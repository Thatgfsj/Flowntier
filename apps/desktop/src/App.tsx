import { useEffect, useRef, useState } from 'react';
import { PhaseTimeline, AgentCard, Card, type PhaseState, type AgentStatus } from '@aco/ui';
import type { WfEvent } from '@aco/shared';
import { TopBar } from './zones/TopBar.js';
import { LeftRoster } from './zones/LeftRoster.js';
// CenterPanel removed: the Plan tab + ReasoningBubble replace the
// dedicated center view (see PR #2).
import { RightPanel } from './zones/RightPanel.js';
import { BottomConsole } from './zones/BottomConsole.js';
import { CommandDock } from './zones/CommandDock.js';
import { Settings } from './zones/Settings.js';
import { ReasoningBubble } from '@aco/ui';
import { ReviewVerdict } from '@aco/ui';

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

// Tasks are populated dynamically from GET /api/workflow/{id}/plan
// after the orchestrator's planning phase completes. The legacy
// hardcoded placeholders were removed in this commit because they
// never reflected what the runtime was actually working on.
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

const RUNTIME_URL = 'http://127.0.0.1:7317';

function nowIso(): string {
  return new Date().toISOString();
}

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
  const [backendMode, setBackendMode] = useState<'real' | 'simulator' | 'unknown'>('unknown');
  const [currentWfId, setCurrentWfId] = useState<string | null>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const busyRef = useRef(false);
  // Expose for screenshot / debug scripts.
  useEffect(() => {
    // @ts-expect-error: window.__acoCurrentWfId is a debug hook
    window.__acoCurrentWfId = currentWfId;
  }, [currentWfId]);

  // Whenever a new workflow starts, poll /plan until it's ready,
  // then replace the task list with the real tasks. Falls back to
  // appending rows as task_status events arrive (see applyEvent).
  useEffect(() => {
    if (!currentWfId) return;
    let cancelled = false;
    let attempts = 0;
    const poll = async () => {
      while (!cancelled && attempts < 60) {
        attempts += 1;
        try {
          const r = await fetch(`${RUNTIME_URL}/api/workflow/${currentWfId}/plan`);
          if (r.ok) {
            const data = await r.json();
            if (cancelled) return;
            if (data?.status === 'ready' && data.parsed_plan?.nodes) {
              const rows: TaskRow[] = data.parsed_plan.nodes.map(
                (n: { id: string; title: string; owner_role?: string }) => ({
                  id: n.id,
                  title: n.title,
                  owner: n.owner_role ?? '',
                  fileHint: undefined,
                  state: 'PENDING',
                }),
              );
              setTasks(rows);
              return;
            }
          }
        } catch {
          // fall through and retry
        }
        await new Promise((r) => setTimeout(r, 1000));
      }
    };
    void poll();
    return () => {
      cancelled = true;
    };
  }, [currentWfId]);

  // Probe the runtime on mount to decide real vs simulator.
  useEffect(() => {
    void (async () => {
      try {
        const r = await fetch(`${RUNTIME_URL}/api/state`, { signal: AbortSignal.timeout(2000) });
        if (r.ok) {
          setBackendMode('real');
          return;
        }
      } catch {
        // fall through
      }
      setBackendMode('simulator');
    })();
  }, []);

  const reset = () => {
    if (wsRef.current) {
      wsRef.current.close();
      wsRef.current = null;
    }
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
  };

  const applyEvent = (event: WfEvent) => {
    setEvents((prev) => [...prev, event]);
    if (event.kind === 'transition' && event.to) {
      const idx = PHASES.findIndex((p) => p.name === event.to);
      if (idx >= 0) {
        setActivePhase(idx);
        setPhaseStates((prev) => ({ ...prev, [event.to as Phase['name']]: 'done' }));
        // mark earlier phases done
        setPhaseStates((prev) => {
          const next = { ...prev };
          for (let i = 0; i < idx; i++) {
            const phaseName = PHASES[i]?.name;
            if (phaseName) {
              next[phaseName] = 'done';
            }
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
      // Map agent_id → status
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
      // Update the matching row's state in place. If the task isn't
      // in the list yet (orchestrator emitted before we fetched
      // /plan), append a fresh row.
      const newState = event.task_status;
      const summary = event.task_summary;
      // Build the new row conditionally — exactOptionalPropertyTypes
      // complains about setting `summary: undefined` on a property
      // typed `summary?: string`. Omit the key when undefined.
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
        // Append a row using the event's task_title as the title.
        const fresh: TaskRow = summary
          ? {
              id: event.task_id!,
              title: event.task_title ?? event.task_id!,
              owner: '',
              state: newState,
              summary,
            }
          : {
              id: event.task_id!,
              title: event.task_title ?? event.task_id!,
              owner: '',
              state: newState,
            };
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

    // Open the WebSocket with auto-reconnect. If the socket dies
    // mid-run, the polling loop will still detect completion; the
    // reconnect lets us catch any *late* events for visual feedback.
    let ws: WebSocket | null = null;
    let reconnectAttempts = 0;
    const connect = () => {
      ws = new WebSocket(`ws://127.0.0.1:7317/api/events/stream`);
      wsRef.current = ws;
      ws.onmessage = (ev) => {
        try {
          const data = JSON.parse(ev.data) as Record<string, unknown>;
          if (data.kind === 'heartbeat') return;
          applyEvent(data as unknown as WfEvent);
        } catch (e) {
          console.warn('bad event', e);
        }
      };
      ws.onclose = () => {
        if (busyRef.current && reconnectAttempts < 5) {
          reconnectAttempts += 1;
          const delay = Math.min(500 * 2 ** reconnectAttempts, 5_000);
          window.setTimeout(connect, delay);
        }
      };
      ws.onerror = () => {
        // onclose will follow
      };
    };
    connect();

    // POST the workflow
    try {
      const resp = await fetch(`${RUNTIME_URL}/api/workflow`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ text, speed: 'fast' }),
      });
      const data = (await resp.json()) as { id: string };
      setCurrentWfId(data.id);

      // Poll for completion
      const poll = async () => {
        try {
          const r = await fetch(`${RUNTIME_URL}/api/workflow/${data.id}/summary`);
          if (r.ok) {
            const summary = (await r.json()) as { summary: string };
            setFinalReport(summary.summary);
            setReviewVerdict({ verdict: 'PASS', summary: '工作流已完成' });
            busyRef.current = false;
            setBusy(false);
            setCompleted(true);
            return;
          }
        } catch {
          // ignore
        }
        window.setTimeout(() => void poll(), 1000);
      };
      void poll();
    } catch (e) {
      console.warn('workflow POST failed', e);
      busyRef.current = false;
      setBusy(false);
    }
  };

  const handleSubmit = () => {
    if (completed) {
      reset();
      return;
    }
    const text = cmd.trim() || '实现 POST /auth/login 接口';
    setCmd('');
    if (backendMode === 'real') {
      void startRealWorkflow(text);
    } else {
      // Simulator fallback not implemented in this revision; show
      // a console line and stay in idle.
      setEvents((prev) => [
        ...prev,
        {
          kind: 'console',
          ts: nowIso(),
          agent_id: 'agent:system',
          level: 'warn',
          message: `runtime 未启动，请先运行 apps/runtime（python -m aco_runtime.main）`,
        },
      ]);
    }
  };

  useEffect(() => {
    return () => {
      wsRef.current?.close();
    };
  }, []);

  return (
    <div className="flex h-screen flex-col">
      <TopBar
        projectName="Agent Company OS"
        subtitle={
          completed
            ? '上次工作流已完成'
            : busy
              ? '运行中…'
              : backendMode === 'simulator'
                ? '未连接 runtime（启动 Python 端以启用 AI）'
                : '示例工作流：实现登录接口'
        }
        onSettingsClick={() => setSettingsOpen(true)}
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

          {(
            <AgentCard
              role="chief"
              name="首席代理"
              status={agentStatusToRole(agentStatus.chief)}
              subtitle={
                agentStatus.chief === 'thinking'
                  ? '沉稳的策略师 · 正在分析'
                  : agentStatus.chief === 'speaking'
                    ? '沉稳的策略师 · 正在汇报'
                    : '沉稳的策略师 · 待命'
              }
              progress={0.5}
            />
          )}

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
        {...(completed ? { resetLabel: '重置' } : {})}
      />

      <BottomConsole events={events} />

      <Settings open={settingsOpen} onClose={() => setSettingsOpen(false)} />
    </div>
  );
}
