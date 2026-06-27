import { useTranslation } from 'react-i18next';
import { AgentCard, type AgentStatus } from '@flowntier/ui';

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
 * The chief/critic/worker agent names use the same i18n keys
 * as perTask.agent.* so they stay in sync with PerTaskConsole.
 */
export function LeftRoster({
  chiefStatus,
  criticAStatus,
  criticBStatus,
  workerStatus,
}: LeftRosterProps) {
  const { t } = useTranslation();
  return (
    <div className="flex flex-col gap-2">
      <h2 className="px-1 text-xs font-semibold uppercase tracking-wide text-text-secondary">
        {t('perTask.agent.chief')}
      </h2>
      <AgentCard
        role="chief"
        name={t('perTask.agent.chief')}
        status={chiefStatus}
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
        subtitle={t('leftRoster.criticASubtitle')}
      />
      <AgentCard
        role="critic-b"
        name={t('perTask.agent.criticB')}
        status={criticBStatus}
        subtitle={t('leftRoster.criticBSubtitle')}
      />

      <h2 className="mt-3 px-1 text-xs font-semibold uppercase tracking-wide text-text-secondary">
        {t('perTask.agent.worker')}
      </h2>
      <AgentCard
        role="worker"
        name={t('perTask.agent.worker')}
        status={workerStatus}
        subtitle={t('leftRoster.workerSubtitle')}
      />
    </div>
  );
}