import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Card, ReasoningBubble, ReviewVerdict } from '@flowntier/ui';

export interface CenterPanelProps {
  chiefCard: ReactNode;
  /**
   * True when a workflow is currently running OR just finished.
   * When false, we render an empty-state guidance card instead
   * of the demo reasoning content.
   */
  hasActiveWorkflow: boolean;
  /**
   * Optional callback for the empty-state "Try sample" button.
   * If absent, the button is hidden (e.g. during loading).
   */
  onTrySample?: (() => void) | undefined;
}

/**
 * Z3 — center panel. Current reasoning / review / task.
 *
 * Two modes:
 *   hasActiveWorkflow=true  : show live chief + reviewer output.
 *   hasActiveWorkflow=false : show an empty-state guidance card
 *                              ("no workflow yet, here's how to
 *                              start one") with a "Try sample"
 *                              shortcut.
 */
export function CenterPanel({ chiefCard, hasActiveWorkflow, onTrySample }: CenterPanelProps) {
  const { t } = useTranslation();
  if (!hasActiveWorkflow) {
    return (
      <div className="flex flex-col gap-3">
        <Card>
          <div className="flex flex-col items-start gap-3 py-6 text-center">
            <div className="self-center text-3xl">▶</div>
            <h3 className="text-base font-semibold text-text-primary">{t('centerPanel.emptyTitle')}</h3>
            <p className="text-sm text-text-secondary">
              {t('centerPanel.emptyHint')}
            </p>
            <ul className="self-start space-y-1 text-left text-sm text-text-secondary">
              <li>• <span className="font-mono text-xs">{t('centerPanel.exampleAddTests')}</span></li>
              <li>• <span className="font-mono text-xs">{t('centerPanel.exampleAuth')}</span></li>
              <li>• <span className="font-mono text-xs">{t('centerPanel.exampleRefactor')}</span></li>
            </ul>
            {onTrySample && (
              <button
                type="button"
                onClick={onTrySample}
                className="mt-2 self-center rounded-md bg-accent px-4 py-2 text-sm font-medium text-white transition-opacity hover:opacity-90"
              >
                {t('centerPanel.orTrySample')}
              </button>
            )}
          </div>
        </Card>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-3">
      {chiefCard}

      <ReasoningBubble
        agentName={t('perTask.agent.chief')}
        roleColorClass="border-t-chief"
        step={t('centerPanel.activeStep')}
        body={t('centerPanel.activeBody')}
        ago={t('centerPanel.agoSeconds', { seconds: 2 })}
      />

      <Card>
        <h3 className="mb-2 text-sm font-semibold">{t('centerPanel.reviewHeading')}</h3>
        <ReviewVerdict
          verdict="PASS"
          confidence={0.87}
          issues={[]}
          summary={t('centerPanel.reviewSummary')}
        />
      </Card>
    </div>
  );
}
