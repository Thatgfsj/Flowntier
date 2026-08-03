import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { check as checkUpdaterPlugin } from '@tauri-apps/plugin-updater';
import { checkForUpdate, installUpdate, type UpdateBanner } from './lib/updater';
import { kvGet, kvSet } from './lib/api.js';
import { Welcome } from './components/Welcome';
import { WorkdirSetup } from './components/WorkdirSetup';
import { PhaseTimeline, AgentCard, Card, type PhaseState } from '@flowntier/ui';
import { TopBar } from './zones/TopBar.js';
import { CenterPanel } from './zones/CenterPanel.js';
import { LeftRoster } from './zones/LeftRoster.js';
import { RightPanel } from './zones/RightPanel.js';
import { BottomConsole } from './zones/BottomConsole.js';
import { CommandDock } from './zones/CommandDock.js';
import { Settings } from './zones/Settings.js';
import { ReasoningBubble } from '@flowntier/ui';
import { ReviewVerdict } from '@flowntier/ui';
import { useWorkflowState } from './hooks/useWorkflowState.js';
import { invoke } from '@tauri-apps/api/core';
import { ChatZone } from './zones/ChatZone.js';
import {
  WorkflowProvider,
  useTicker,
  useWorkflowEventStream,
  useWorkflow,
  useTasks,
  useEvents,
  usePhaseStates,
  useMilestones,
} from './contexts/WorkflowContext.js';
import { DisabledModelsProvider } from './contexts/DisabledModelsContext.js';
import type { PhaseName } from './contexts/workflowReducer.js';
import { PHASES } from './contexts/workflowReducer.js';

// v0.4.29 (Phase A): PHASES + the phase/agent types now
// live in `apps/desktop/src/contexts/workflowReducer.ts`
// so the reducer is the single source of truth. The reducer
// re-imports them where it needs them; App.tsx and the
// TSX subcomponents use the same imports via
// `import { PHASES } from './contexts/workflowReducer.js'`
// (see the top of file).

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
  // v0.4.29 (Phase A): the workbench is rendered inside a
  // <WorkflowProvider>. Gate screens (firstRun / workdir)
  // stay outside the provider because they don't need
  // workflow state — they short-circuit the render before
  // any reducer plumbing is needed.
  const { t } = useTranslation();
  const [firstRun, setFirstRun] = useState<boolean | null>(null);
  const [workdir, setWorkdir] = useState<string | null>(null);
  const [workdirReady, setWorkdirReady] = useState(false);
  const [workdirSkipped, setWorkdirSkipped] = useState(false);

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
        console.warn('[App] kv_get(first_run) failed; defaulting to dashboard:', e);
        setFirstRun(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    void (async () => {
      try {
        const r = await invoke<{ k: string; v: unknown }>('kv_get', { key: 'workdir_skipped' });
        if (r && r.v === true) setWorkdirSkipped(true);
      } catch {}
    })();
  }, []);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const r = await invoke<string | null>('get_workdir');
        if (cancelled) return;
        if (r === null || r === '') {
          setWorkdir(null);
        } else {
          setWorkdir(r);
        }
      } catch (e) {
        console.warn('[App] get_workdir failed; defaulting to dashboard:', e);
        setWorkdir(null);
      } finally {
        if (!cancelled) setWorkdirReady(true);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  // Step 0: workdir not yet checked.
  if (!workdirReady) {
    return <div className="h-screen w-screen bg-surface-1" />;
  }
  // Step 1: workdir not set.
  if (workdir === null && !workdirSkipped) {
    return (
      <WorkdirSetup
        initialPath=""
        mode="first-launch"
        onConfirm={async (p) => {
          try {
            await invoke('set_workdir_with_nwt', { path: p });
            setWorkdir(p);
          } catch (e) {
            console.error('[App] set_workdir_with_nwt failed:', e);
            alert(t('app.workdirWriteFailed', { error: String(e) }));
          }
        }}
        onSkip={async () => {
          try { await invoke('kv_set', { key: 'workdir_skipped', value: true }); } catch {}
          try { await invoke('clear_workdir'); } catch {}
          setWorkdirSkipped(true);
        }}
      />
    );
  }

  // Step 2: first-run gate.
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
    <WorkflowProvider>
      <DisabledModelsProvider>
        <WorkbenchApp workdir={workdir} />
      </DisabledModelsProvider>
    </WorkflowProvider>
  );
// v0.4.29 (Phase A): the workbench body. All workflow state
// now lives in the reducer (WorkflowContext); this component
// only owns the App-only state (settings modal, chat panel,
// cmd input, recent history, drift banner, update banner)
// plus the cancellation/state-coupling handlers.

function parseSemver(s: string): number[] | null {
  const parts = s.split('.').map((n) => parseInt(n, 10));
  if (parts.some((n) => Number.isNaN(n))) return null;
  return parts;
}
const at = (arr: number[], i: number): number => arr[i] ?? 0;
function isSidecarDrift(sidecar: string, min: string): boolean {
  const a = parseSemver(sidecar);
  const b = parseSemver(min);
  if (a === null || b === null) return false;
  return at(a, 0) < at(b, 0) ||
    (at(a, 0) === at(b, 0) && at(a, 1) < at(b, 1)) ||
    (at(a, 0) === at(b, 0) && at(a, 1) === at(b, 1) && at(a, 2) < at(b, 2));
}

function WorkbenchApp({ workdir }: { workdir: string | null }) {
  const { t } = useTranslation();
  const { state, dispatch } = useWorkflow();

  // Mount the event stream subscription + the idle-timer ticker
  // here so they live on the workbench's lifecycle (App-level
  // gate screens don't need them).
  useWorkflowEventStream();
  useTicker();

  // ── App-only state (persists across workflow runs) ─────────────
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [chatOpen, setChatOpen] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [cmd, setCmd] = useState('');
  const [updateBanner, setUpdateBanner] = useState<UpdateBanner>({ available: false });
  const [drift, setDrift] = useState<
    | { detected: false }
    | { detected: true; sidecar: string; min_compatible: string }
  >({ detected: false });
  const [recentCmds, setRecentCmds] = useState<string[]>(() => {
    try {
      const raw = localStorage.getItem('flowntier.cmd_history');
      if (!raw) return [];
      const parsed = JSON.parse(raw);
      return Array.isArray(parsed) ? parsed.slice(0, 50) : [];
    } catch { return []; }
  });

  // ── Reducer-driven slices (re-render only when these change) ──
  const tasks = useTasks();
  const events = useEvents();
  const phaseStates = usePhaseStates();
  const milestones = useMilestones();
  const {
    busy,
    completed,
    currentWfId,
    activePhase,
    agentStatus,
    workflowError,
    reviewVerdict,
    reviewerVerdicts,
    finalReport,
  } = state;

  // ── Effects (boots + IPC) ──────────────────────────────────────
  // BUG-FRONTEND-RT-?? (event 000046): close any open modal when
  // the language toggle fires (TopBar dispatches the
  // 'flowntier:close-modals' event).
  useEffect(() => {
    const handler = () => setSettingsOpen(false);
    window.addEventListener('flowntier:close-modals', handler);
    return () => window.removeEventListener('flowntier:close-modals', handler);
  }, []);

  // BUG-FRONTEND-RT-17 (event 000045): seed env-var API keys on
  // startup.
  useEffect(() => {
    void (async () => {
      try { await invoke('seed_secrets'); } catch (e) {
        console.warn('[App] seed_secrets failed:', e);
      }
    })();
  }, []);

  // Sidecar version handshake.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const r = await invoke<{ sidecar: string; min_compatible: string }>('rpc_version');
        if (cancelled) return;
        if (!isSidecarDrift(r.sidecar, r.min_compatible)) return;
        const dismissedFor = await kvGet<string>('drift_dismissed_for_version');
        if (cancelled) return;
        if (dismissedFor === r.sidecar) return;
        setDrift({ detected: true, sidecar: r.sidecar, min_compatible: r.min_compatible });
      } catch (e) {
        console.warn('[flowntier] rpc_version check threw:', e);
      }
    })();
    return () => { cancelled = true; };
  }, []);

  // Update check.
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
    return () => { cancelled = true; };
  }, []);

  // Debug hook for screenshots.
  useEffect(() => {
    // @ts-expect-error: window.__flowntierCurrentWfId is a debug hook
    window.__flowntierCurrentWfId = currentWfId;
  }, [currentWfId]);

  // ── Polling ────────────────────────────────────────────────────
  const workflowSummary = useWorkflowState(currentWfId);

  // ── Handlers ───────────────────────────────────────────────────
  const handleCancel = async () => {
    if (!currentWfId || cancelling) return;
    setCancelling(true);
    try {
      const { cancelWorkflow } = await import('./lib/api.js');
      await cancelWorkflow(currentWfId);
    } catch (e) {
      console.warn('[flowntier] cancel_workflow failed:', e);
    } finally {
      dispatch({ type: 'SET_BUSY', value: false });
      setCancelling(false);
    }
  };

  const startRealWorkflow = async (text: string) => {
    dispatch({ type: 'START_WORKFLOW' });
    try {
      if (workdir && workdir.length > 0) {
        try { await kvSet('nwt_root', workdir); } catch (e) {
          console.warn('[App] kv_set(nwt_root) failed:', e);
        }
      }
      const data = await invoke<{ id: string }>('start_workflow_cmd', { text });
      dispatch({ type: 'SET_CURRENT_WF_ID', id: data.id });

      // 30-min watchdog polls /api/workflow/{id} for summary.
      const deadline = Date.now() + 30 * 60 * 1000;
      while (Date.now() < deadline) {
        await new Promise(r => setTimeout(r, 2000));
        if (!state.busy) break;
        try {
          const wf = await invoke<Record<string, unknown>>('get_workflow', { id: data.id });
          if (wf && wf.summary) {
            dispatch({ type: 'SET_FINAL_REPORT', report: wf.summary as string });
            if (state.busy) {
              dispatch({ type: 'SET_BUSY', value: false });
              dispatch({ type: 'SET_COMPLETED', value: true });
            }
            break;
          }
        } catch { /* continue */ }
      }

      if (!state.completed) {
        const err = state.workflowError;
        if (err !== null) {
          dispatch({ type: 'SET_REVIEW_VERDICT', verdict: { verdict: 'REPAIR', summary: err } });
        } else {
          dispatch({ type: 'SET_REVIEW_VERDICT', verdict: { verdict: 'REPAIR', summary: t('workflow.verdict.timeout') } });
        }
      } else if (state.workflowError !== null) {
        dispatch({ type: 'SET_REVIEW_VERDICT', verdict: { verdict: 'REPAIR', summary: state.workflowError } });
      }
    } catch (e) {
      console.warn('workflow failed', e);
      dispatch({ type: 'SET_REVIEW_VERDICT', verdict: { verdict: 'REPAIR', summary: `工作流启动失败: ${e}` } });
    } finally {
      dispatch({ type: 'SET_BUSY', value: false });
      dispatch({ type: 'SET_COMPLETED', value: true });
    }
  };

  const handleSubmit = () => {
    if (state.completed) {
      dispatch({ type: 'RESET' });
      return;
    }
    const text = cmd.trim() || t('workflow.cmd.fallback');
    setCmd('');
    setRecentCmds((prev) => {
      const next = [text, ...prev.filter((c) => c !== text)].slice(0, 50);
      try {
        localStorage.setItem('flowntier.cmd_history', JSON.stringify(next));
      } catch (e) {
        console.warn('[App] cmd history persist failed (quota?):', e);
      }
      return next;
    });
    void startRealWorkflow(text);
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
        // v0.4.22 (event 000118, fix 7): Stop button. Only
        // render the handler when busy AND we have a wf_id so
        // the modal stops showing once the workflow finishes.
        // Spread conditionally because exactOptionalPropertyTypes
        // rejects `onCancel={undefined}`.
        {...(busy && currentWfId ? { onCancel: handleCancel } : {})}
        cancelling={cancelling}
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
          className="w-[260px] shrink-0 overflow-y-auto border-r border-border-strong bg-surface-3 p-2"
          aria-label={t('app.aria.roster')}
        >
          <LeftRoster />
        </aside>

        <main role="main" aria-label={t('app.aria.workspace')} className="flex-1 overflow-y-auto p-3">
          <div className="mb-3 rounded-lg border border-border bg-surface-2 shadow-sm">
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
                if (idx >= 0) dispatch({type: 'SET_ACTIVE_PHASE', index: idx});
              }}
            />
          </div>

          {/* v0.4.22 (event 000118): show the user what task they
              submitted AND where in the 8-phase pipeline the
              orchestrator currently is. Hidden when no workflow
              has ever been started so the empty-state isn't
              polluted. */}
          {workflowSummary?.userRequest && (
            <TaskProgressPanel summary={workflowSummary} phases={PHASES} phaseStates={phaseStates} />
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
                  status={agentStatus.chief.status}
                  statusLabel={t(`agentCard.status.${agentStatus.chief.status}`)}
                  subtitle={
                    agentStatus.chief.status === 'thinking'
                      ? t('roster.chief.thinking')
                      : agentStatus.chief.status === 'speaking'
                        ? t('roster.chief.speaking')
                        : t('roster.chief.idle')
                  }
                  progress={busy ? 0.5 : undefined}
                />

                <ReasoningBubble
                  agentName={t('perTask.agent.chief')}
                  roleColorClass="border-t-chief"
                  // v0.4.22 (event 000118): previous text was
                  // hard-coded as `phases.delivery + idx/8`, which
                  // always read "交付 N / 8" regardless of the
                  // actual phase. Show the real label for the
                  // current active phase from the PHASES table.
                  step={`${PHASES[activePhase]?.label ?? t('phases.requirement')} ${activePhase + 1} / ${PHASES.length}`}
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
                  {reviewVerdict ? (
                    <ReviewVerdict
                      verdict={reviewVerdict.verdict}
                      verdictLabel={t(`reviewVerdict.verdict.${reviewVerdict.verdict}`)}
                      // v0.4.22 (event 000112): display the real
                      // confidence from the last reviewer_verdict
                      // event (currently 0.0 placeholder). Falls
                      // back to 1.00 when no critic has reported
                      // yet and reviewVerdict was set by the
                      // final-review binding path.
                      confidenceLabel={t('reviewVerdict.confidence', {
                        value: (() => {
                          const last = [...reviewerVerdicts]
                            .reverse()
                            .find((r) => r.phase === 'final-review');
                          if (last && last.confidence > 0) {
                            return last.confidence.toFixed(2);
                          }
                          return '1.00';
                        })()
                      })}
                      confidence={
                        (() => {
                          const last = [...reviewerVerdicts]
                            .reverse()
                            .find((r) => r.phase === 'final-review');
                          return last && last.confidence > 0
                            ? last.confidence
                            : 1;
                        })()
                      }
                      issues={
                        // v0.4.22 (event 000112): the orchestrator
                        // emits a list of plain strings (no severity),
                        // but ReviewVerdict expects structured
                        // ReviewIssue with severity. Mapping plain
                        // → MAJOR strips semantics, so keep an
                        // empty list until event 000115 (structured
                        // JSON reviewer prompt) gives us severity.
                        []
                      }
                      summary={reviewVerdict.summary}
                    />
                  ) : (
                    <div className="flex flex-col gap-1 text-xs text-text-secondary">
                      <div className="flex items-center gap-2">
                        <span className="inline-block h-2 w-2 animate-pulse rounded-full bg-text-tertiary" />
                        <span>{t('centerPanel.reviewPending', { defaultValue: '等待审查员 B 评审…' })}</span>
                      </div>
                      <p className="mt-1 text-[11px] text-text-tertiary">
                        {t('centerPanel.reviewPendingHint', {
                          defaultValue: '完成 8 个交付阶段后会自动出评审意见；当前未生成。'
                        })}
                      </p>
                    </div>
                  )}
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

          {/* v0.4.29 (Phase E): milestones demoted to a
              collapsible footer card so the main column isn't
              visually fragmented when the workflow is long.
              Shows the count + last 3 in the header, full list
              when expanded. */}
          {milestones.length > 0 && (
            <details className="rounded-lg border border-border bg-surface-2 p-2 shadow-sm">
              <summary className="cursor-pointer select-none text-xs font-semibold uppercase tracking-wide text-text-secondary">
                {t('app.milestones.title', { defaultValue: '里程碑' })}
                <span className="ml-2 font-mono normal-case text-text-tertiary">
                  ({milestones.length})
                </span>
              </summary>
              <ul className="mt-2 flex flex-col gap-0.5 text-primary">
                {milestones.map((m, i) => (
                  <li key={i} className="font-mono text-xs">▸ {m}</li>
                ))}
              </ul>
            </details>
          )}
        </main>

        <aside
          className="w-[360px] shrink-0 overflow-y-auto border-l border-border-strong bg-surface-3 p-3"
          aria-label={t('app.aria.tasks')}
        >
          <RightPanel tasks={tasks} events={events} />
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

      {/* v0.3 ChatZone — progressive. Collapsed by default; toggle via TopBar.
          event 000110 (fix A): collapse button moved INTO ChatZone header
          (see ChatZone.tsx) so the top-right corner no longer carries
          two visually overlapping buttons ("清空" + floating "▾ 折叠").
          event 000110 (fix B): collapsed bar's subtitle now mirrors
          the expanded header so users don't see two different taglines
          for the same panel. */}
      <div
        className={`relative flex shrink-0 border-t border-border transition-[height] ${
          chatOpen ? 'h-[480px]' : 'h-9'
        }`}
      >
        {chatOpen ? (
          <div className="h-full w-full">
            <ChatZone
              onCollapse={() => setChatOpen(false)}
              // v0.4.22 (event 000118, fix 2): the bottom
              // "当前就绪 + 工具" block needs to know which head
              // role (chief / critic-a / critic-b) is currently
              // driving the workflow. We derive it from the
              // current phase + the role agentStatus. Per the
              // chairman: don't surface the worker-only case.
              activeAgent={(() => {
                if (!busy) return null;
                const headRoles: ReadonlyArray<{
                  role: 'chief' | 'critic-a' | 'critic-b';
                  i18nKey: 'chief' | 'criticA' | 'criticB';
                  phases: ReadonlyArray<PhaseName>;
                }> = [
                  { role: 'chief', i18nKey: 'chief', phases: ['requirement', 'plan', 'dispatch', 'repair', 'delivery'] },
                  { role: 'critic-a', i18nKey: 'criticA', phases: ['plan-review', 'final-review'] },
                  { role: 'critic-b', i18nKey: 'criticB', phases: ['plan-review', 'final-review'] },
                ];
                for (const def of headRoles) {
                  // Find the first phase in `def.phases` that's
                  // currently `active`; that phase's label is
                  // what the chairman sees in the "当前就绪"
                  // block header.
                  let activePhaseLabel: string | null = null;
                  for (const p of def.phases) {
                    if (phaseStates[p] === 'active') {
                      activePhaseLabel = PHASES.find((pp) => pp.name === p)?.label ?? p;
                      break;
                    }
                  }
                  if (!activePhaseLabel) continue;
                  // Map AgentStatus ('idle' | 'thinking' | 'speaking'
                  // | 'error') onto ActiveAgentInfo's narrower
                  // union. ChatZone only cares about live activity,
                  // so collapse 'error' -> 'idle' for display.
                  const raw = agentStatus[def.role].status;
                  const status: 'idle' | 'thinking' | 'speaking' =
                    raw === 'thinking' || raw === 'speaking' ? raw : 'idle';
                  return {
                    role: def.role,
                    label: t(`chatZone.roles.${def.i18nKey}`),
                    status,
                    phaseLabel: activePhaseLabel,
                  };
                }
                return null;
              })()}
            />
          </div>
        ) : (
          <button
            type="button"
            onClick={() => setChatOpen(true)}
            className="flex h-9 w-full items-center justify-between gap-2 bg-surface-2 px-4 text-left text-xs text-text-secondary hover:bg-surface-1"
            aria-label={t('app.aria.chatExpand')}
          >
            <span className="font-mono">ChatZone ▸</span>
            <span>{t('chatZone.subtitle', { defaultValue: '由设置 → 角色 → 模型 分配驱动' })}</span>
          </button>
        )}
      </div>

      <BottomConsole />

      <Settings open={settingsOpen} onClose={() => setSettingsOpen(false)} workdir={workdir} />
    </div>
  );
}

// ── v0.4.22 (event 000118): TaskProgressPanel ───────────────────────
//
// Surfaces two pieces of context the user couldn't see before:
//
//   1. The actual task they asked the agent to do (the raw
//      `user_request` text the orchestrator stored when
//      `start_workflow` was called). The previous UI only showed
//      "1-需求" / "5-开发" labels — users had no idea what the
//      current workflow was actually trying to accomplish.
//
//   2. A live progress bar driven by the per-phase `PhaseState`
//      map. 8 dots, one per phase; the active one pulses. Below
//      the dots we show a compact "X / 8 phase · Y tasks done"
//      counter so the user can see forward motion at a glance.
};

interface TaskProgressPanelProps {
  summary: import('./hooks/useWorkflowState.js').WorkflowSummary;
  phases: ReadonlyArray<{ name: PhaseName; label: string }>;
  phaseStates: Record<PhaseName, PhaseState>;
}

function TaskProgressPanel({ summary, phases, phaseStates }: TaskProgressPanelProps) {
  const { t } = useTranslation();
  // v0.4.22 (event 000118, follow-up): even though
  // useWorkflowState already coalesces user_request/tasks_* to
  // ''/0, this component also defends against summary itself
  // being a half-populated object (e.g. Tauri IPC returns a
  // barebones shape on certain pipe failures). Every field is
  // read with a safeStr/safeNum guard so a missing key never
  // becomes `undefined.includes(...)` somewhere downstream.
  const req = typeof summary.userRequest === 'string' ? summary.userRequest : '';
  const reqShort = req.length > 240 ? req.slice(0, 240) + '…' : req;
  const tasksDone = typeof summary.tasksDone === 'number' ? summary.tasksDone : 0;
  const tasksTotal = typeof summary.tasksTotal === 'number' ? summary.tasksTotal : 0;
  // v0.4.22 (follow-up): phaseStates can be missing keys if a
  // phase is added server-side but not yet known to this
  // frontend build. Default every lookup to 'pending'.
  const getPhaseState = (name: PhaseName): PhaseState => phaseStates[name] ?? 'pending';
  const done = phases.filter((p) => getPhaseState(p.name) === 'done').length;
  const phaseNameLabel: Record<PhaseName, string> = {
    requirement: t('phases.requirement'),
    plan: t('phases.planning'),
    'plan-review': t('phases.plan_review'),
    dispatch: t('phases.dispatch'),
    develop: t('phases.development'),
    'final-review': t('phases.review'),
    repair: t('phases.repair'),
    delivery: t('phases.delivery'),
  };
  // The orchestrator's `phase` field is the prefixed string
  // "1-requirement" / "2-plan" / etc. — strip the leading "N-"
  // when matching against PhaseName. Defensive: summary.phase
  // can be undefined on the very first poll, and the previous
  // version passed `summary.phase || ''` straight into the
  // String.prototype.replace call which was fine, but the findIndex
  // callback was not null-safe on `p` itself.
  const activePhaseName =
    typeof summary.phase === 'string' && summary.phase.length > 0
      ? summary.phase.replace(/^\d+-/, '')
      : '';
  const activeIdx = phases.findIndex((p) => p && (p.name === activePhaseName || p.name === summary.phase));
  // activePhase is guarded everywhere it is dereferenced below.
  const activePhase = activeIdx >= 0 ? phases[activeIdx] : undefined;
  // v0.4.22 (event 000118): live elapsed counter for the
  // active phase. Updates every 1s so the user can SEE that
  // the workflow isn't hung — without it, a 2-minute develop
  // phase looks indistinguishable from a stuck run, and the
  // user concludes "workflow timed out". Use a ref-driven
  // tick so React doesn't re-render the whole panel just to
  // refresh the counter.
  const [elapsedSec, setElapsedSec] = useState(0);
  useEffect(() => {
    setElapsedSec(0);
    const startedAt = Date.now();
    const id = window.setInterval(() => {
      setElapsedSec(Math.floor((Date.now() - startedAt) / 1000));
    }, 1000);
    return () => window.clearInterval(id);
  }, [activeIdx]);
  const elapsedLabel =
    elapsedSec < 60
      ? `${elapsedSec}s`
      : `${Math.floor(elapsedSec / 60)}m${elapsedSec % 60}s`;
  return (
    <Card className="!p-3">
      <div className="mb-2 flex items-start gap-3">
        <div className="shrink-0 rounded-md bg-chief/10 px-2 py-1 font-mono text-[10px] uppercase tracking-wide text-chief">
          {t('workbench.currentTask')}
        </div>
        <div className="min-w-0 flex-1">
          <p
            className="break-words text-sm text-text-primary"
            title={req}
          >
            {reqShort || t('workbench.noTask')}
          </p>
        </div>
      </div>

      {/* 8-phase progress strip */}
      <div className="mb-2 flex items-center gap-1">
        {phases.map((p, i) => {
          const s = getPhaseState(p.name);
          const isActive = i === activeIdx;
          const cls =
            s === 'done'
              ? 'bg-status-done'
              : isActive
                ? 'bg-chief flt-anim-pulse'
                : 'bg-surface-3';
          return (
            <div
              key={p.name}
              className={`h-1.5 flex-1 rounded-full transition-colors ${cls}`}
              title={`${p.label}${isActive ? ' · 当前' : ''}`}
            />
          );
        })}
      </div>

      <div className="flex items-center justify-between text-[10px] text-text-secondary">
        <span>
          {t('workbench.phaseProgress', {
            current: activePhase?.label ?? '?',
            done,
            total: phases.length,
          })}
        </span>
        {tasksTotal > 0 && (
          <span>
            {t('workbench.taskProgress', { done: tasksDone, total: tasksTotal })}
          </span>
        )}
        {activePhase && (
          <span className="ml-auto font-mono">
            {t('workbench.phaseName', { name: phaseNameLabel[activePhase.name] ?? activePhase.name })}
          </span>
        )}
      </div>
      {/*
        v0.4.22 (event 000118): live elapsed counter for the
        active phase. Placed on its own line so it doesn't get
        truncated on narrow screens. Resets to 0 when the
        phase changes (activeIdx effect above).
      */}
      {activePhase && getPhaseState(activePhase.name) !== 'done' && (
        <div className="mt-1 font-mono text-[10px] text-text-secondary">
          {t('workbench.elapsed', { time: elapsedLabel })}
        </div>
      )}
    </Card>
  );
}
