import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Card } from "@flowntier/ui";
import type { PhaseState } from "@flowntier/ui";
import type { PhaseName } from "../contexts/workflowReducer.js";
import type { WorkflowSummary } from "../hooks/useWorkflowState.js";

export interface TaskProgressPanelProps {
  summary: WorkflowSummary;
  phases: ReadonlyArray<{ name: PhaseName; label: string }>;
  phaseStates: Record<PhaseName, PhaseState>;
}

export function TaskProgressPanel({ summary, phases, phaseStates }: TaskProgressPanelProps) {
  const { t } = useTranslation();
  // v0.4.22 (event 000118, follow-up): even though
  // useWorkflowState already coalesces user_request/tasks_* to
  // ''/0, this component also defends against summary itself
  // being a half-populated object (e.g. Tauri IPC returns a
  // barebones shape on certain pipe failures). Every field is
  // read with a safeStr/safeNum guard so a missing key never
  // becomes `undefined.includes(...)` somewhere downstream.
  const req = typeof summary.userRequest === "string" ? summary.userRequest : "";
  const reqShort = req.length > 240 ? req.slice(0, 240) + "…" : req;
  const tasksDone = typeof summary.tasksDone === "number" ? summary.tasksDone : 0;
  const tasksTotal = typeof summary.tasksTotal === "number" ? summary.tasksTotal : 0;
  // v0.4.22 (follow-up): phaseStates can be missing keys if a
  // phase is added server-side but not yet known to this
  // frontend build. Default every lookup to 'pending'.
  const getPhaseState = (name: PhaseName): PhaseState => phaseStates[name] ?? "pending";
  const done = phases.filter((p) => getPhaseState(p.name) === "done").length;
  const phaseNameLabel: Record<PhaseName, string> = {
    requirement: t("phases.requirement"),
    plan: t("phases.planning"),
    "plan-review": t("phases.plan_review"),
    dispatch: t("phases.dispatch"),
    develop: t("phases.development"),
    "final-review": t("phases.review"),
    repair: t("phases.repair"),
    delivery: t("phases.delivery"),
  };
  // The orchestrator's `phase` field is the prefixed string
  // "1-requirement" / "2-plan" / etc. — strip the leading "N-"
  // when matching against PhaseName. Defensive: summary.phase
  // can be undefined on the very first poll, and the previous
  // version passed `summary.phase || ''` straight into the
  // String.prototype.replace call which was fine, but the findIndex
  // callback was not null-safe on `p` itself.
  const activePhaseName =
    typeof summary.phase === "string" && summary.phase.length > 0
      ? summary.phase.replace(/^\d+-/, "")
      : "";
  const activeIdx = phases.findIndex(
    (p) => p && (p.name === activePhaseName || p.name === summary.phase),
  );
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
    elapsedSec < 60 ? `${elapsedSec}s` : `${Math.floor(elapsedSec / 60)}m${elapsedSec % 60}s`;
  return (
    <Card className="!p-3">
      <div className="mb-2 flex items-start gap-3">
        <div className="shrink-0 rounded-md bg-chief/10 px-2 py-1 font-mono text-[10px] uppercase tracking-wide text-chief">
          {t("workbench.currentTask")}
        </div>
        <div className="min-w-0 flex-1">
          <p className="break-words text-sm text-text-primary" title={req}>
            {reqShort || t("workbench.noTask")}
          </p>
        </div>
      </div>

      {/* 8-phase progress strip */}
      <div className="mb-2 flex items-center gap-1">
        {phases.map((p, i) => {
          const s = getPhaseState(p.name);
          const isActive = i === activeIdx;
          const cls =
            s === "done" ? "bg-status-done" : isActive ? "bg-chief flt-anim-pulse" : "bg-surface-3";
          return (
            <div
              key={p.name}
              className={`h-1.5 flex-1 rounded-full transition-colors ${cls}`}
              title={`${p.label}${isActive ? " · 当前" : ""}`}
            />
          );
        })}
      </div>

      <div className="flex items-center justify-between text-[10px] text-text-secondary">
        <span>
          {t("workbench.phaseProgress", {
            current: activePhase?.label ?? "?",
            done,
            total: phases.length,
          })}
        </span>
        {tasksTotal > 0 && (
          <span>{t("workbench.taskProgress", { done: tasksDone, total: tasksTotal })}</span>
        )}
        {activePhase && (
          <span className="ml-auto font-mono">
            {t("workbench.phaseName", {
              name: phaseNameLabel[activePhase.name] ?? activePhase.name,
            })}
          </span>
        )}
      </div>
      {/*
        v0.4.22 (event 000118): live elapsed counter for the
        active phase. Placed on its own line so it doesn't get
        truncated on narrow screens. Resets to 0 when the
        phase changes (activeIdx effect above).
      */}
      {activePhase && getPhaseState(activePhase.name) !== "done" && (
        <div className="mt-1 font-mono text-[10px] text-text-secondary">
          {t("workbench.elapsed", { time: elapsedLabel })}
        </div>
      )}
    </Card>
  );
}
