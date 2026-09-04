import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Card, ReasoningBubble, ReviewVerdict } from "@flowntier/ui";

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
        <Card className="border-white/10 bg-surface-2/95 shadow-lg">
          <div className="flex flex-col items-center gap-4 py-8 text-center max-w-lg mx-auto">
            <div className="flex h-12 w-12 items-center justify-center rounded-2xl bg-gradient-to-br from-chief/20 to-blue-500/10 border border-chief/30 text-chief shadow-lg shadow-chief/15">
              <svg className="h-6 w-6" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={2}
                  d="M13 10V3L4 14h7v7l9-11h-7z"
                />
              </svg>
            </div>
            <div>
              <h3 className="text-base font-semibold text-white tracking-tight">
                {t("centerPanel.emptyTitle")}
              </h3>
              <p className="mt-1 text-xs text-text-secondary">{t("centerPanel.emptyHint")}</p>
            </div>
            <div className="w-full flex flex-col gap-1.5 rounded-xl border border-white/5 bg-surface-1/80 p-3 text-left">
              <span className="text-[11px] font-medium uppercase tracking-wider text-text-tertiary px-1">
                示例指令
              </span>
              <div className="space-y-1">
                <div className="rounded-lg bg-surface-2/60 px-2.5 py-1.5 font-mono text-xs text-text-secondary border border-white/5">
                  ▸ {t("centerPanel.exampleAddTests")}
                </div>
                <div className="rounded-lg bg-surface-2/60 px-2.5 py-1.5 font-mono text-xs text-text-secondary border border-white/5">
                  ▸ {t("centerPanel.exampleAuth")}
                </div>
                <div className="rounded-lg bg-surface-2/60 px-2.5 py-1.5 font-mono text-xs text-text-secondary border border-white/5">
                  ▸ {t("centerPanel.exampleRefactor")}
                </div>
              </div>
            </div>
            {onTrySample && (
              <button
                type="button"
                onClick={onTrySample}
                className="mt-1 flex items-center gap-2 rounded-xl bg-gradient-to-r from-chief to-blue-500 px-5 py-2 text-xs font-semibold text-white shadow-md shadow-chief/25 transition-all hover:brightness-110 active:scale-95"
              >
                <span>{t("centerPanel.orTrySample")}</span>
                <svg className="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    strokeWidth={2}
                    d="M9 5l7 7-7 7"
                  />
                </svg>
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

      {/* BUG-FRONTEND-5 (audit 000026 #80/#81): the previous code
          rendered TWO ReasoningBubble + TWO ReviewVerdict cards
          simultaneously when App.tsx passed chiefCard. App.tsx
          already renders the live versions; this branch is the
          demo/static content shown when chiefCard is null (i.e.
          the placeholder UI). Skip the static bubbles when a
          live chiefCard is supplied. */}
      {!chiefCard && (
        <>
          <ReasoningBubble
            agentName={t("perTask.agent.chief")}
            roleColorClass="border-t-chief"
            step={t("centerPanel.activeStep")}
            body={t("centerPanel.activeBody")}
            ago={t("centerPanel.agoSeconds", { seconds: 2 })}
          />

          <Card>
            <h3 className="mb-2 text-sm font-semibold">{t("centerPanel.reviewHeading")}</h3>
            <ReviewVerdict
              verdict="PASS"
              confidence={0.87}
              issues={[]}
              summary={t("centerPanel.reviewSummary")}
            />
          </Card>
        </>
      )}
    </div>
  );
}
