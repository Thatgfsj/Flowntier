import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { AgentCard, type AgentStatus } from '@flowntier/ui';
import { FileTree } from '../components/FileTree.js';

export interface LeftRosterProps {
  chiefStatus: AgentStatus;
  criticAStatus: AgentStatus;
  criticBStatus: AgentStatus;
  workerStatus: AgentStatus;
  /**
   * v0.4.22 (event 000118, fix 3): per-task worker status
   * map keyed by the orchestrator's `t{idx}` task id. When
   * non-empty, render N worker cards (one per plan task)
   * instead of the single legacy `workerStatus` card. The
   * single card stays as a fallback for the case where the
   * runtime hasn't yet emitted any task-tagged events (e.g.
   * during Phase 1-4 or before Phase 5 dispatches).
   */
  workerTaskStatus?: Record<string, AgentStatus>;
  workerTaskTitles?: Record<string, string>;
}

/**
 * Z2 — left roster. Lists every agent with status.
 *
 * BUG-FRONTEND-RT-4 (event 000030): all user-facing strings
 * were hardcoded Chinese. Now resolved via i18n at render time.
 *
 * v0.4.21 (event 000066): added a second tab "文件" that
 * mounts the new <FileTree /> component. Chairman's
 * "切工作目录不显示新文件" was the trigger — FileTree polls
 * every 5s and shows the live tree under the runtime's
 * current workspace root, so chief writes are visible
 * without a manual refresh.
 */
export function LeftRoster({
  chiefStatus,
  criticAStatus,
  criticBStatus,
  workerStatus,
  workerTaskStatus,
  workerTaskTitles,
}: LeftRosterProps) {
  const { t } = useTranslation();
  const [tab, setTab] = useState<'agents' | 'files'>('agents');
  // v0.4.22 (event 000118, fix 3): if the orchestrator has
  // tagged any Phase-5 events with a `t{idx}` task id,
  // render one card per task. Sort by id so `t0`, `t1`, …
  // stay in plan order rather than event-arrival order
  // (worker N's first TextDelta arrives before worker N-1's).
  const perTaskIds = workerTaskStatus
    ? Object.keys(workerTaskStatus).sort()
    : [];

  return (
    <div className="flex flex-col gap-2">
      <div
        role="tablist"
        aria-label="Left panel sections"
        className="flex gap-1 border-b border-border"
      >
        <button
          type="button"
          role="tab"
          aria-selected={tab === 'agents'}
          onClick={() => setTab('agents')}
          className={
            'px-2 py-1 text-xs ' +
            (tab === 'agents'
              ? 'border-b-2 border-primary font-semibold text-text-primary'
              : 'text-text-secondary hover:text-text-primary')
          }
        >
          角色
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={tab === 'files'}
          onClick={() => setTab('files')}
          className={
            'px-2 py-1 text-xs ' +
            (tab === 'files'
              ? 'border-b-2 border-primary font-semibold text-text-primary'
              : 'text-text-secondary hover:text-text-primary')
          }
        >
          文件
        </button>
      </div>

      {tab === 'agents' && (
        <div className="flex flex-col gap-2">
          <h2 className="px-1 text-xs font-semibold uppercase tracking-wide text-text-secondary">
            {t('perTask.agent.chief')}
          </h2>
          <AgentCard
            role="chief"
            name={t('perTask.agent.chief')}
            status={chiefStatus}
            statusLabel={t(`agentCard.status.${chiefStatus}`)}
            subtitle={t('roster.chief.thinking')}
            progress={chiefStatus === 'thinking' ? 0.5 : undefined}
          />

          <h2 className="mt-3 px-1 text-xs font-semibold uppercase tracking-wide text-text-secondary">
            {t('leftRoster.reviewers')}
          </h2>
          <AgentCard
            role="critic-a"
            name={t('perTask.agent.criticA')}
            status={criticAStatus}
            statusLabel={t(`agentCard.status.${criticAStatus}`)}
            subtitle={t('leftRoster.criticASubtitle')}
          />
          <AgentCard
            role="critic-b"
            name={t('perTask.agent.criticB')}
            status={criticBStatus}
            statusLabel={t(`agentCard.status.${criticBStatus}`)}
            subtitle={t('leftRoster.criticBSubtitle')}
          />

          <h2 className="mt-3 px-1 text-xs font-semibold uppercase tracking-wide text-text-secondary">
            {t('perTask.agent.worker')}
            {perTaskIds.length > 1 && (
              <span className="ml-1 text-text-tertiary">({perTaskIds.length})</span>
            )}
          </h2>
          {perTaskIds.length > 0 ? (
            perTaskIds.map((tid) => {
              const s = workerTaskStatus![tid] ?? 'idle';
              const title = workerTaskTitles?.[tid];
              const display = title
                ? `${t('perTask.agent.worker')} · ${tid}: ${title}`
                : `${t('perTask.agent.worker')} · ${tid}`;
              return (
                <AgentCard
                  key={tid}
                  role="worker"
                  name={display}
                  status={s}
                  statusLabel={t(`agentCard.status.${s}`)}
                  subtitle={t('leftRoster.workerSubtitle')}
                />
              );
            })
          ) : (
            // No per-task events yet — fall back to the
            // legacy single worker card so phases before
            // dispatch still get a status row.
            <AgentCard
              role="worker"
              name={t('perTask.agent.worker')}
              status={workerStatus}
              statusLabel={t(`agentCard.status.${workerStatus}`)}
              subtitle={t('leftRoster.workerSubtitle')}
            />
          )}
        </div>
      )}

      {tab === 'files' && (
        <div className="mt-2">
          <FileTree pollMs={5000} />
        </div>
      )}
    </div>
  );
}