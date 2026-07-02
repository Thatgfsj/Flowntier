import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { AgentCard, type AgentStatus } from '@flowntier/ui';
import { FileTree } from '../components/FileTree.js';

export interface LeftRosterProps {
  chiefStatus: AgentStatus;
  criticAStatus: AgentStatus;
  criticBStatus: AgentStatus;
  workerStatus: AgentStatus;
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
}: LeftRosterProps) {
  const { t } = useTranslation();
  const [tab, setTab] = useState<'agents' | 'files'>('agents');

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
          </h2>
          <AgentCard
            role="worker"
            name={t('perTask.agent.worker')}
            status={workerStatus}
            statusLabel={t(`agentCard.status.${workerStatus}`)}
            subtitle={t('leftRoster.workerSubtitle')}
          />
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