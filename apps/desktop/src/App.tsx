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

// v0.4.22 (event 000068): Phase names match the orchestrator's
// 8-phase state machine (history/PROJECT_SPEC.md). Names must
// stay in sync with `crates/pipe-server/src/orchestrator.rs
// ::PHASES` — the events pipe matches on these strings.
interface Phase {
  name:
    | 'requirement'
    | 'plan'
    | 'plan-review'
    | 'dispatch'
    | 'develop'
    | 'final-review'
    | 'repair'
    | 'delivery';
  label: string;
}

// v0.4.22 (event 000068): PHASES names match the orchestrator's
// 8-phase state machine (history/PROJECT_SPEC.md). The unprefixed
// names are what the PhaseTransition events use (orchestrator.rs
// PHASES const); the labels are i18n'd.
const PHASES: ReadonlyArray<Phase> = [
  { name: 'requirement', label: '1-需求' },
  { name: 'plan', label: '2-规划' },
  { name: 'plan-review', label: '3-计划审核' },
  { name: 'dispatch', label: '4-派发' },
  { name: 'develop', label: '5-开发' },
  { name: 'final-review', label: '6-终审' },
  { name: 'repair', label: '7-修复' },
  { name: 'delivery', label: '8-交付' },
];

const PHASE_STATE: Record<Phase['name'], PhaseState> = {
  requirement: 'pending',
  plan: 'pending',
  'plan-review': 'pending',
  dispatch: 'pending',
  develop: 'pending',
  'final-review': 'pending',
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

/** v0.4.22 (event 000094): map a phase name to the role
 * that runs it. Drives the agent-status dashboard so the
 * chairman sees the right card flip to "thinking" instead of
 * every card staying "空闲" even when the chief is mid-Plan.
 */
function phaseToRole(
  phase: string,
): keyof AgentStatusMap | null {
  switch (phase) {
    case 'requirement':
    case 'plan':
    case 'dispatch':
    case 'repair':
    case 'delivery':
      return 'chief';
    case 'plan-review':
    case 'final-review':
      return 'critic-a';
    case 'develop':
      return 'worker';
    default:
      return null;
  }
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
  // BUG-FRONTEND-RT-4 (event 000030): pull t() out of useTranslation
  // for the App-level JSX (TopBar subtitle, chiefCard, etc.). The
  // DriftBanner child component has its own t() at line 102.
  const { t } = useTranslation();
  const [firstRun, setFirstRun] = useState<boolean | null>(null);
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
  // v0.4.22 (event 000095): the orchestrator emits a Done
  // event with status='FAILED: <reason>' when an agent can't
  // reach the provider (e.g. 401 from Mimo). We capture the
  // last failed status so the dashboard can show "401 Invalid
  // API Key" instead of the misleading "工作流超时".
  const [workflowError, setWorkflowError] = useState<string | null>(null);
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
  // BUG-FRONTEND-RT-?? (event 000046): when the language: when the language
  // toggle in TopBar fires, also close any open modal
  // (e.g. the Settings modal) so the toggle isn't blocked by
  // the modal backdrop. Listens for the custom event
  // 'flowntier:close-modals' that TopBar dispatches.
  useEffect(() => {
    const handler = () => setSettingsOpen(false);
    window.addEventListener('flowntier:close-modals', handler);
    return () => window.removeEventListener('flowntier:close-modals', handler);
  }, []);

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
  // BUG-FRONTEND-RT-9 (event 000039): the workdirSkipped flag
  // is read on mount to remember the user's previous "skip
  // workdir" choice across launches. MUST be declared BEFORE
  // any early-return `if` block, otherwise React throws
  // "Rendered more hooks than during the previous render"
  // (BUG-FRONTEND-RT-10). Hooks must always be called in the
  // same order on every render.
  const [workdirSkipped, setWorkdirSkipped] = useState(false);
  useEffect(() => {
    void (async () => {
      try {
        const r = await invoke<{ k: string; v: unknown }>('kv_get', { key: 'workdir_skipped' });
        if (r && r.v === true) setWorkdirSkipped(true);
      } catch {}
    })();
  }, []);

  // BUG-FRONTEND-RT-17 (event 000045): seed env-var API keys
  // on app startup. The Rust shell's seed_secrets command
  // reads standard env vars (OPENAI_API_KEY, ANTHROPIC_API_KEY,
  // GOOGLE_API_KEY, DEEPSEEK_API_KEY, MOONSHOT_API_KEY,
  // OPEN_BIGMODEL_API_KEY) and stores them in the keychain.
  // Without this, users with env vars set up wouldn't see any
  // providers in Settings — the panel would say (0) even
  // though keys are available.
  useEffect(() => {
    void (async () => {
      try {
        await invoke('seed_secrets');
      } catch (e) {
        console.warn('[App] seed_secrets failed:', e);
      }
    })();
  }, []);
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
    setWorkflowError(null);
    setCurrentWfId(null);
    setPlanNodes([]);
    setPlanEdges([]);
    setShowPlanGraph(false);
  };

  const applyEvent = (event: WfEvent) => {
    setEvents((prev) => [...prev, event]);
    // BUG-FRONTEND-RT-3 fix (event 000029): the previous code
    // only unblocked the cmd bar when the polling loop saw
    // `wf.summary`. If the Rust side hangs (or crashed without
    // writing summary), the busy flag stayed true for the full
    // 10-minute deadline. Now any completion-style event
    // (workflow_complete, all milestones reached, or the
    // explicit `workflow_done` synthetic event the agent emits
    // when the report is finalized) immediately resets busy so
    // the user can start a new workflow.
    const isCompletion =
      event.kind === 'workflow_complete' ||
      // Fallback: last milestone is delivery = "done"
      (event.kind === 'milestone' &&
       (event as { phase?: string }).phase === 'delivery' &&
       (event as { status?: string }).status === 'completed');
    if (isCompletion) {
      busyRef.current = false;
      setBusy(false);
      setCompleted(true);
      setReviewVerdict({ verdict: 'PASS', summary: t('workflow.verdict.pass') });
    }
    // v0.4.22 (event 000095): detect a globally-failed workflow.
    // The orchestrator emits Done { status: 'FAILED: <reason>' }
    // when every agent candidate failed (e.g. 401 from Mimo,
    // 30-min timeout, etc.). Capture the last failure status so
    // the user sees the real error instead of the generic
    // "10-minute workflow timeout" the watchdog would show.
    if (event.kind === 'done' && typeof event.status === 'string'
        && event.status.startsWith('FAILED')) {
      const summary = (event.summary ?? '') as string;
      // Strip the "FAILED: " prefix for display
      const clean = event.status.replace(/^FAILED:\s*/, '');
      setWorkflowError(`${clean}\n\n${summary}`.trim());
    }
    if ((event.kind === 'transition' || (event as { kind?: string }).kind === 'phase_transition') && (event as { to?: string }).to) {
      const to = (event as { to: string }).to;
      const idx = PHASES.findIndex((p) => p.name === to);
      if (idx >= 0) {
        setActivePhase(idx);
        setPhaseStates((prev) => {
          const next = { ...prev };
          for (let i = 0; i < idx; i++) {
            const phaseName = PHASES[i]?.name;
            if (phaseName) next[phaseName] = 'done';
          }
          const toName = to as Phase['name'];
          next[toName] = 'active';
          return next;
        });
        // v0.4.22 (event 000094): flip the corresponding role
        // card to "thinking" when its phase becomes active.
        // Without this the dashboard shows every role as
        // 空闲 even mid-workflow, because no per-agent
        // event fires during the 8-phase orchestrator runs.
        // Chief runs phases 1-requirement, 2-plan, 4-dispatch,
        // 7-repair, 8-delivery. Critic-a + critic-b run
        // 3-plan-review + 6-final-review. Worker runs 5-develop.
        const role = phaseToRole(to);
        if (role) {
          setAgentStatus((prev) => ({ ...prev, [role]: 'thinking' }));
        }
      }
    }
    if (event.kind === 'milestone' && event.label) {
      setMilestones((prev) => [...prev, event.label]);
    }
    // BUG-FRONTEND-RT-14 (event 000043): the previous code
    // only updated phaseStates for `transition` events. But
    // the agent emits `milestone` events (status: started /
    // completed). Without this branch, the 8-phase timeline
    // dots never change — all 8 stay empty. We now also update
    // phaseStates on milestone events: status='completed' marks
    // the phase as 'done'; status='started' marks it as 'active'.
    if (event.kind === 'milestone' && event.phase) {
      const idx = PHASES.findIndex((p) => p.name === event.phase);
      if (idx >= 0) {
        const phaseName = event.phase as Phase['name'];
        const ms = event as { status?: string };
        const newState: PhaseState =
          ms.status === 'completed' ? 'done' :
          ms.status === 'started' || ms.status === 'in_progress' ? 'active' :
          'pending';
        setActivePhase(newState === 'active' ? idx : activePhase);
        setPhaseStates((prev) => ({ ...prev, [phaseName]: newState }));
        // v0.4.22 (event 000094): flip role back to idle on
        // phase completion (or keep thinking if still active).
        const role = phaseToRole(event.phase);
        if (role) {
          setAgentStatus((prev) => ({
            ...prev,
            [role]: newState === 'done' ? 'idle' : 'thinking',
          }));
        }
      }
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
    setWorkflowError(null);
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

      // Poll for completion using invoke. The busy flag can also
      // be flipped false by an event handler (see applyEvent's
      // completion branch) — we early-break in that case too.
      const deadline = Date.now() + 600000;
      while (Date.now() < deadline) {
        await new Promise(r => setTimeout(r, 2000));
        if (!busyRef.current) break;  // event handler completed us
        try {
          const wf = await invoke<Record<string, unknown>>('get_workflow', { id: data.id });
          if (wf && wf.summary) {
            setFinalReport(wf.summary as string);
            if (busyRef.current) {
              busyRef.current = false;
              setBusy(false);
              setCompleted(true);
            }
            break;
          }
        } catch {
          // continue polling
        }
      }

      if (!completed) {
        // v0.4.22 (event 000095): prefer the captured
        // workflowError (real 401 message) over the generic
        // "10-minute workflow timeout" placeholder.
        if (workflowError !== null) {
          setReviewVerdict({ verdict: 'REPAIR', summary: workflowError });
        } else {
          setReviewVerdict({ verdict: 'REPAIR', summary: t('workflow.verdict.timeout') });
        }
      } else if (workflowError !== null) {
        // Completed normally but every agent failed (rare — fall
        // through to the delivery phase anyway). Surface the
        // error so the chairman can see what went wrong.
        setReviewVerdict({ verdict: 'REPAIR', summary: workflowError });
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
    const text = cmd.trim() || t('workflow.cmd.fallback');
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
  // for now (advanced users); if skipped, we record the skip
  // intent in kv and route the user to the dashboard anyway
  // (workdir stays null so the agent knows there's no project).
  if (workdir === null && !workdirSkipped) {
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
            // BUG-FRONTEND-RT-8 (event 000039): surface the
            // error to the user so they know the data dir write
            // failed. Previously we just `console.error`d and the
            // dialog dismissed silently, leaving the user confused
            // about why nothing happened.
            alert(t('app.workdirWriteFailed', { error: String(e) }));
          }
        }}
        onSkip={async () => {
          // BUG-FRONTEND-RT-9 (event 000039): the previous
          // `setWorkdir(null)` put the app in an infinite re-render
          // loop because the WorkdirSetup dialog kept appearing
          // on every render when workdir was null. Now we record
          // the skip intent in kv (so it survives reload) and
          // set `workdirSkipped` to route the user to the
          // dashboard despite the null workdir.
          try { await invoke('kv_set', { key: 'workdir_skipped', value: true }); } catch {}
          try { await invoke('clear_workdir'); } catch {}
          setWorkdirSkipped(true);
          // Do NOT setWorkdir(null) — workdir stays null, but the
          // skip flag tells the render condition to bypass the
          // WorkdirSetup dialog and go straight to the dashboard.
        }}
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
            ? t('topbar.status.done')
            : busy
              ? t('topbar.status.busy')
              : t('topbar.status.idle')
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
          aria-label={t('app.aria.roster')}
        >
          <LeftRoster
            chiefStatus={agentStatusToRole(agentStatus.chief)}
            criticAStatus={agentStatusToRole(agentStatus['critic-a'])}
            criticBStatus={agentStatusToRole(agentStatus['critic-b'])}
            workerStatus={agentStatusToRole(agentStatus.worker)}
          />
        </aside>

        <main role="main" aria-label={t('app.aria.workspace')} className="flex-1 overflow-y-auto p-3">
          <div className="mb-3 rounded-lg border border-border bg-surface-1 p-2">
            <PhaseTimeline
              steps={PHASES.map((p) => ({
                name: p.name,
                // BUG-FRONTEND-RT-4 (event 000030): phase labels
                // were hardcoded Chinese. Now resolved via i18n at
                // render time. PHASES itself keeps the names as
                // stable keys for event correlation.
                label: t(`phases.${p.name}`),
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
                      // BUG-FRONTEND-3 (audit 000026 #14): the
                      // previous code only invoked start_workflow_cmd
                      // and then did nothing — busy/phase states
                      // never updated, so the dashboard appeared
                      // unchanged after clicking. Now delegate to
                      // the same startRealWorkflow path the cmd
                      // bar uses, so the sample workflow gets the
                      // exact same UI treatment as a real command.
                      await startRealWorkflow(wf.user_request);
                    } catch (e) {
                      console.warn('[App] onTrySample failed:', e);
                    }
                  }
            }
            chiefCard={
              <>
                <AgentCard
                  role="chief"
                  name={t('perTask.agent.chief')}
                  status={agentStatusToRole(agentStatus.chief)}
                  statusLabel={t(`agentCard.status.${agentStatusToRole(agentStatus.chief)}`)}
                  subtitle={
                    agentStatus.chief === 'thinking'
                      ? t('roster.chief.thinking')
                      : agentStatus.chief === 'speaking'
                        ? t('roster.chief.speaking')
                        : t('roster.chief.idle')
                  }
                  progress={busy ? 0.5 : undefined}
                />

                <ReasoningBubble
                  agentName={t('perTask.agent.chief')}
                  roleColorClass="border-t-chief"
                  step={`${t('phases.delivery')} ${activePhase + 1} / 8`}
                  body={
                    completed
                      ? t('workflow.status.done')
                      : busy
                        ? t('workflow.status.running')
                        : t('workflow.status.idle')
                  }
                  ago={busy ? t('roster.chief.speaking') : t('roster.chief.idle')}
                />

                <Card>
                  <h3 className="mb-2 text-sm font-semibold">审核员 B — 架构审查</h3>
<ReviewVerdict
                verdict="PASS"
                verdictLabel={t('reviewVerdict.verdict.PASS')}
                confidenceLabel={t('reviewVerdict.confidence', { value: '0.87' })}
                confidence={0.87}
                issues={[]}
                summary={t('centerPanel.reviewSummary')}
              />
                </Card>
              </>
            }
          />

          {workflowError !== null && (
            <Card className="border-status-error/60 bg-status-error/10">
              <h3 className="mb-2 text-sm font-semibold text-status-error">
                {t('workflow.error.heading', { defaultValue: 'Workflow Error' })}
              </h3>
              <pre className="whitespace-pre-wrap break-words font-mono text-xs text-status-error">
                {workflowError}
              </pre>
              <p className="mt-2 text-xs text-text-secondary">
                {t('workflow.error.hint', {
                  defaultValue: 'Most common cause: the API key stored for this role has expired or is wrong. Open Settings → Providers → MiMo and re-save the key.',
                })}
              </p>
            </Card>
          )}

          {reviewVerdict !== null && (
            <Card>
              <h3 className="mb-2 text-sm font-semibold">{t('app.finalReview')}</h3>
              <ReviewVerdict
                verdict={reviewVerdict.verdict}
                verdictLabel={t(`reviewVerdict.verdict.${reviewVerdict.verdict}`)}
                confidenceLabel={t('reviewVerdict.confidence', { value: '1.00' })}
                severityLabels={{
                  MAJOR: t('reviewVerdict.severity.MAJOR'),
                  MINOR: t('reviewVerdict.severity.MINOR'),
                  NIT: t('reviewVerdict.severity.NIT'),
                }}
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
          aria-label={t('app.aria.tasks')}
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
        {...(completed ? { resetLabel: t('app.reset') } : {})}
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
              aria-label={t('app.aria.chatCollapse')}
            >
              {t('app.chatCollapse')}
            </button>
          </>
        ) : (
          <button
            type="button"
            onClick={() => setChatOpen(true)}
            className="flex h-9 w-full items-center justify-between gap-2 bg-surface-2 px-4 text-left text-xs text-text-secondary hover:bg-surface-1"
            aria-label={t('app.aria.chatExpand')}
          >
            <span className="font-mono">ChatZone ▸</span>
            <span>{t('app.chatExpand')}</span>
          </button>
        )}
      </div>

      <BottomConsole events={events} />

      <Settings open={settingsOpen} onClose={() => setSettingsOpen(false)} workdir={workdir} />
    </div>
  );
}
