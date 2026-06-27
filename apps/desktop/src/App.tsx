import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { check as checkUpdaterPlugin } from '@tauri-apps/plugin-updater';
import { checkForUpdate, installUpdate, type UpdateBanner } from './lib/updater';
import { kvGet, kvSet } from './lib/api.js';
import { Welcome } from './components/Welcome';
import { WorkdirSetup } from './components/WorkdirSetup';
import { PhaseTimeline, AgentCard, Card, type PhaseState, type AgentStatus } from '@flowntier/ui';
import type { WfEvent } from '@flowntier/shared';
import { TopBar } from './zones/TopBar.js';
import { CenterPanel } from './zones/CenterPanel.js';
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

// ── DriftBanner ────────────────────────────────────────────────
// Renders a non-blocking warning at the top of the dashboard
// when the sidecar's reported version is older than the shell's
// expected min_compatible. Common cause: user upgraded the shell
// but the sidecar binary in apps/desktop/src-tauri/binaries/
// is stale (rare in installed builds, common in dev).
interface DriftBannerProps {
  sidecar: string;
  minCompatible: string;
  onDismiss: () => void;
}

function DriftBanner({ sidecar, minCompatible, onDismiss }: DriftBannerProps) {
  const { t } = useTranslation();
  return (
    <div
      role="alert"
      className="flex items-center justify-between gap-4 border-b border-error bg-error/15 px-4 py-2 text-xs text-primary"
    >
      <span>
        {t('drift.message', { sidecar, expected: minCompatible })}
      </span>
      <button
        type="button"
        onClick={onDismiss}
        className="rounded-md border border-error/40 px-2 py-0.5 text-xs text-error hover:bg-error/25"
      >
        {t('drift.dismiss')}
      </button>
    </div>
  );
}

export function App() {
  const [activePhase, setActivePhase] = useState(0);
  const [phaseStates, setPhaseStates] = useState<Record<Phase['name'], PhaseState>>({ ...PHASE_STATE });
  const [tasks, setTasks] = useState<TaskRow[]>([...INITIAL_TASKS]);
  const [agentStatus, setAgentStatus] = useState<AgentStatusMap>({ ...INITIAL_AGENT_STATUS });
  const [events, setEvents] = useState<WfEvent[]>([]);
  const [cmd, setCmd] = useState('');
  // Recent command history (persisted in localStorage so it
  // survives quit+relaunch). Most-recent first, capped at 50.
  // recentCmds is loaded from localStorage lazily; setRecentCmds is
  // not used at the call site (the only call site uses the setter
  // inside a callback which TS lint does not detect). Suppress the
  // unused warning by underscore-prefixing the import.
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  const [recentCmds, setRecentCmds] = useState<string[]>(() => {
    try {
      const raw = localStorage.getItem('flowntier.cmd_history');
      if (!raw) return [];
      const parsed = JSON.parse(raw);
      return Array.isArray(parsed) ? parsed.slice(0, 50) : [];
    } catch { return []; }
  });
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
  const [updateBanner, setUpdateBanner] = useState<UpdateBanner>({ available: false });
  const [planNodes, setPlanNodes] = useState<PlanTaskNode[]>([]);
  const [planEdges, setPlanEdges] = useState<PlanEdge[]>([]);
  const [selectedTask, setSelectedTask] = useState<string | null>(null);
  const [showPlanGraph, setShowPlanGraph] = useState(false);
  const [chatOpen, setChatOpen] = useState(false);
  const busyRef = useRef(false);

  // v0.4: first-run gate. Reads the kv table on mount; if
  // first_run is true (default) we render <Welcome> instead
  // of the main dashboard. Once the user clicks "进入工作台"
  // Welcome calls first_run_complete which writes false and
  // calls onComplete -> setFirstRun(false).
  const [firstRun, setFirstRun] = useState<boolean | null>(null);
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const r = await invoke<{ k: string; v: unknown }>('kv_get', {
          key: 'first_run',
        });
        if (cancelled) return;
        const isFirst =
          !r || r.v === null || r.v === 'true' || r.v === true;
        setFirstRun(isFirst);
      } catch (e) {
        // If the call fails (e.g. backend not ready yet), default
        // to the main dashboard rather than blocking the user.
        console.warn('[App] kv_get(first_run) failed; defaulting to dashboard:', e);
        setFirstRun(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  // v0.4-NWT: workspace workdir. Read on mount; if null, show
  // the WorkdirSetup full-screen dialog before the main dashboard.
  // The dialog is mandatory on first launch (the AI can't create
  // project sub-directories without a workdir to put them in).
  const [workdir, setWorkdir] = useState<string | null>(null);
  const [workdirReady, setWorkdirReady] = useState(false);
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const r = await invoke<string | null>('get_workdir');
        if (cancelled) return;
        // BUG-019 fix (event 000023): treat BOTH `null` (no
        // workdir.json yet) and empty string `""` (workdir.json
        // exists but value is empty) as "needs workdir". Previously
        // an empty string silently let the dashboard render with
        // no workdir set, which made every subsequent nwt_log
        // fail. Now both cases trigger the WorkdirSetup dialog.
        if (r === null || r === '') {
          setWorkdir(null);
        } else {
          setWorkdir(r);
        }
      } catch (e) {
        console.warn('[App] get_workdir failed; defaulting to dashboard:', e);
        // Even on error, treat as "needs workdir" — the user
        // re-runs setup. Safer than rendering a dashboard with
        // no workdir behind it.
        setWorkdir(null);
      } finally {
        if (!cancelled) setWorkdirReady(true);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

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

  // Check for updates once on app start. Non-blocking; result goes
  // into the updateBanner state and is rendered by the TopBar.
  // See apps/desktop/src/lib/updater.ts for the wrapper.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const banner = await checkForUpdate();
        if (!cancelled) setUpdateBanner(banner);
      } catch (e) {
        console.warn('[flowntier] update check threw:', e);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  // v0.4: sidecar version handshake. Calls rpc_version once on
  // mount; if the sidecar's reported version is < min_compatible
  // (read from its own response), we render a non-blocking
  // DriftBanner above the dashboard.
  //
  // Non-fatal: if the call fails we just log and continue. The
  // most common cause is the sidecar binary not yet attached to
  // its named pipe; we don't want a startup race to flash a
  // banner on every launch.
  const [drift, setDrift] = useState<
    | { detected: false }
    | {
        detected: true;
        sidecar: string;
        min_compatible: string;
      }
  >({ detected: false });
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const r = await invoke<{
          sidecar: string;
          min_compatible: string;
        }>('rpc_version');
        if (cancelled) return;
        // Simple semver comparison: split on '.', compare ints.
        // BUG-027 fix (event 000025): bail out if either version
        // contains a non-numeric segment, instead of silently
        // treating malformed input as 0 (which always triggers
        // the drift banner). The sidecar is expected to return
        // strict semver; if it doesn't, we just don't show the
        // banner.
        const parse = (s: string): number[] | null => {
          const parts = s.split('.').map((n) => parseInt(n, 10));
          if (parts.some((n) => Number.isNaN(n))) return null;
          return parts;
        };
        const a = parse(r.sidecar);
        const b = parse(r.min_compatible);
        if (a === null || b === null) {
          console.warn('[App] rpc_version returned non-semver:', r);
          return;
        }
        const v = (arr: number[], i: number): number => arr[i] ?? 0;
        // Strict less-than: sidecar < min_compatible.
        const isDrift =
          v(a, 0) < v(b, 0) ||
          (v(a, 0) === v(b, 0) && v(a, 1) < v(b, 1)) ||
          (v(a, 0) === v(b, 0) && v(a, 1) === v(b, 1) && v(a, 2) < v(b, 2));
        if (!isDrift) return;
        // Persistent dismiss: re-show only if the user hasn't
        // already dismissed this exact sidecar version. If the
        // sidecar is upgraded in the future (or downgraded to a
        // different version), the banner returns.
        const dismissedFor = await kvGet<string>('drift_dismissed_for_version');
        if (cancelled) return;
        if (dismissedFor === r.sidecar) {
          console.info(
            '[flowntier] drift banner suppressed (user dismissed for this sidecar version)',
          );
          return;
        }
        setDrift({
          detected: true,
          sidecar: r.sidecar,
          min_compatible: r.min_compatible,
        });
      } catch (e) {
        console.warn('[flowntier] rpc_version check threw:', e);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

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
      // NWT Step F: also call the Rust-side set_nwt_root on
      // every workflow start so the agent loop (which uses
      // global state, not React state) has the right path.
      if (workdir && workdir.length > 0) {
        try { await kvSet('nwt_root', workdir); } catch (e) {
          console.warn('[App] kv_set(nwt_root) failed:', e);
        }
      }

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
    // Push to recent-commands history (most-recent first, deduped,
    // capped at 50). localStorage so it survives quit+relaunch.
    setRecentCmds((prev) => {
      const next = [text, ...prev.filter((c) => c !== text)].slice(0, 50);
      try {
        localStorage.setItem('flowntier.cmd_history', JSON.stringify(next));
      } catch (e) {
        // BUG-025 fix (event 000024): localStorage quota (~5 MB)
        // is small enough that an over-eager history could
        // exceed it. Previously we swallowed the error; now we
        // warn so power users hitting the cap can see it.
        console.warn('[App] cmd history persist failed (quota?):', e);
      }
      return next;
    });
    void startRealWorkflow(text);
  };

  // Step 0: workdir not yet checked.
  if (!workdirReady) {
    return <div className="h-screen w-screen bg-surface-1" />;
  }
  // Step 1: workdir not set. Show the WorkdirSetup dialog (mandatory
  // on first launch). User can either pick a directory or skip
  // for now (advanced users); if skipped, workdir is "" and the
  // dialog re-shows on next launch.
  if (workdir === null) {
    return (
      <WorkdirSetup
        initialPath=""
        mode="first-launch"
        onConfirm={async (p) => {
          // BUG-016 fix (event 000022): we now use a SINGLE
          // command `set_workdir_with_nwt` that atomically
          // (a) initialises `.nwt/` in the workdir and
          // (b) writes `workdir.json` to the app data dir.
          // If either step fails, NEITHER is persisted — so the
          // user sees the dialog again on next launch instead of
          // a corrupt half-initialised workspace.
          try {
            await invoke('set_workdir_with_nwt', { path: p });
            setWorkdir(p);
          } catch (e) {
            console.error('[App] set_workdir_with_nwt failed:', e);
          }
        }}
        onSkip={() => setWorkdir('')}
      />
    );
  }
  // Step 2: first-run gate. Show Welcome until the user clicks
  // "进入工作台". firstRun === null means we're still loading
  // the kv value; show a blank screen rather than a flash of
  // the dashboard.
  if (firstRun === null) {
    return <div className="h-screen w-screen bg-surface-1" />;
  }
  if (firstRun) {
    return (
      <Welcome
        onComplete={() => {
          setFirstRun(false);
        }}
      />
    );
  }

  return (
    <div className="flex h-screen flex-col">
      {drift.detected && (
        <DriftBanner
          sidecar={drift.sidecar}
          minCompatible={drift.min_compatible}
          onDismiss={() => {
            // Persist the dismissal keyed by the sidecar version
            // we just showed. If the user upgrades the sidecar
            // and the new version is still older than expected,
            // the banner re-appears.
            void kvSet('drift_dismissed_for_version', drift.sidecar);
            setDrift({ detected: false });
          }}
        />
      )}
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
        updateBanner={updateBanner}
        onUpdateClick={() => {
          // The user clicked the "update available" banner. Re-check
          // (in case cache expired) then install. installUpdate()
          // shows the confirm dialog itself.
          void (async () => {
            try {
              const upd = await checkUpdaterPlugin();
              if (upd) await installUpdate(upd);
              // If install succeeds, downloadAndInstall() will
              // trigger a relaunch; we don't need to update state.
              setUpdateBanner({ available: false });
            } catch (e) {
              console.warn('[flowntier] install failed:', e);
            }
          })();
        }}
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

        <main role="main" aria-label="工作区" className="flex-1 overflow-y-auto p-3">
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

          {/* CenterPanel: empty-state vs live chief+reviewer. */}
          <CenterPanel
            hasActiveWorkflow={busy || milestones.length > 0 || currentWfId !== null}
            onTrySample={
              busy || milestones.length > 0 || currentWfId !== null
                ? undefined
                : async () => {
                    try {
                      const wf = await invoke<{
                        user_request: string;
                        display_name: string;
                      }>('load_sample_workflow');
                      // BUG-022 fix (event 000020): the command name
                      // is `start_workflow_cmd`, not `start_workflow`,
                      // and the args are flat `{ text }` not wrapped
                      // in `{ request: { text } }` (per lib.rs:539).
                      await invoke('start_workflow_cmd', {
                        text: wf.user_request,
                      });
                    } catch (e) {
                      console.warn('[App] onTrySample failed:', e);
                    }
                  }
            }
            chiefCard={
              <>
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
                        ? '正在执行当前阶段…（完整规划在左侧 8 阶段时间线上）'
                        : '等待用户在下方的命令栏输入指令。'
                  }
                  ago={busy ? '正在运行' : '空闲'}
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
              </>
            }
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
        recent={recentCmds}
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

      <Settings open={settingsOpen} onClose={() => setSettingsOpen(false)} workdir={workdir} />
    </div>
  );
}
