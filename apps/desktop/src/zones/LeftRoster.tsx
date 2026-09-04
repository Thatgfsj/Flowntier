import { useState } from "react";
import { useTranslation } from "react-i18next";
import { AgentCard, type AgentStatus } from "@flowntier/ui";
import { FileTree } from "../components/FileTree.js";
import { useAgentStatus, useEvents } from "../contexts/WorkflowContext.js";
import { agentIdToHeadRole } from "../lib/agentId.js";
import { AgentLivePanelTooltip } from "./AgentLivePanel.js";

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
 *
 * v0.4.29 (Phase A): `workerTaskStatus` is now derived inside
 * the component from the event stream (text_delta /
 * tool_started / tool_finished with `task_id`). Previously
 * App.tsx kept a separate `useState` for it and passed it
 * down as a prop — which required App.tsx to re-render on
 * every event. Now the roster subscribes to `useEvents()`
 * directly so changes are scoped to this subtree.
 */
export function LeftRoster() {
  const { t } = useTranslation();
  const [tab, setTab] = useState<"agents" | "files">("agents");

  const agentStatus = useAgentStatus();
  // v0.4.29 (Phase A): derive per-task worker status from the
  // event stream. The runtime tags every worker event with
  // `task_id` ∈ {t0, t1, …} so we can render one card per task.
  const events = useEvents();
  const workerTaskStatus: Record<string, AgentStatus> = {};
  const workerTaskTitles: Record<string, string> = {};
  // Walk the events back-to-front keeping the LAST status / title
  // seen per task id. This is O(n) per render but `events` is
  // bounded by `events.slice(-200)` upstream.
  for (let i = events.length - 1; i >= 0; i--) {
    const e = events[i];
    if (!e) continue;
    if (e.kind === "task_status" && e.task_id) {
      if (workerTaskTitles[e.task_id] === undefined && e.task_title) {
        workerTaskTitles[e.task_id] = e.task_title;
      }
      continue;
    }
    if (e.kind === "text_delta" || e.kind === "tool_started" || e.kind === "tool_finished") {
      const tid = e.task_id;
      if (tid && workerTaskStatus[tid] === undefined) {
        workerTaskStatus[tid] = e.kind === "tool_started" ? "thinking" : "speaking";
      }
    }
  }
  const perTaskIds = Object.keys(workerTaskStatus).sort();

  return (
    <div className="flex flex-col gap-2">
      <div
        role="tablist"
        aria-label="Left panel sections"
        className="flex rounded-lg border border-white/8 bg-surface-2/90 p-1 shadow-inner backdrop-blur-sm"
      >
        <button
          type="button"
          role="tab"
          aria-selected={tab === "agents"}
          onClick={() => setTab("agents")}
          className={
            "flex-1 rounded-md py-1 text-center text-xs font-medium transition-all " +
            (tab === "agents"
              ? "bg-chief/20 text-white shadow-sm shadow-chief/20 border border-chief/40"
              : "text-text-tertiary hover:text-text-primary")
          }
        >
          智能体
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={tab === "files"}
          onClick={() => setTab("files")}
          className={
            "flex-1 rounded-md py-1 text-center text-xs font-medium transition-all " +
            (tab === "files"
              ? "bg-chief/20 text-white shadow-sm shadow-chief/20 border border-chief/40"
              : "text-text-tertiary hover:text-text-primary")
          }
        >
          工作区文件
        </button>
      </div>

      {tab === "agents" && (
        <div className="flex flex-col gap-2">
          <h2 className="px-1 text-xs font-semibold uppercase tracking-wide text-text-secondary">
            {t("perTask.agent.chief")}
          </h2>
          <AgentLivePanelTooltip role="chief">
            <AgentCard
              role="chief"
              name={t("perTask.agent.chief")}
              status={agentStatus.chief.status}
              statusLabel={t(`agentCard.status.${agentStatus.chief.status}`)}
              subtitle={t("roster.chief.thinking")}
              progress={agentStatus.chief.status === "thinking" ? 0.5 : undefined}
            />
          </AgentLivePanelTooltip>

          <h2 className="mt-3 px-1 text-xs font-semibold uppercase tracking-wide text-text-secondary">
            {t("leftRoster.reviewers")}
          </h2>
          <AgentLivePanelTooltip role="critic-a">
            <AgentCard
              role="critic-a"
              name={t("perTask.agent.criticA")}
              status={agentStatus["critic-a"].status}
              statusLabel={t(`agentCard.status.${agentStatus["critic-a"].status}`)}
              subtitle={t("leftRoster.criticASubtitle")}
            />
          </AgentLivePanelTooltip>
          <AgentLivePanelTooltip role="critic-b">
            <AgentCard
              role="critic-b"
              name={t("perTask.agent.criticB")}
              status={agentStatus["critic-b"].status}
              statusLabel={t(`agentCard.status.${agentStatus["critic-b"].status}`)}
              subtitle={t("leftRoster.criticBSubtitle")}
            />
          </AgentLivePanelTooltip>

          <h2 className="mt-3 px-1 text-xs font-semibold uppercase tracking-wide text-text-secondary">
            {t("perTask.agent.worker")}
            {perTaskIds.length > 1 && (
              <span className="ml-1 text-text-tertiary">({perTaskIds.length})</span>
            )}
          </h2>
          {perTaskIds.length > 0 ? (
            perTaskIds.map((tid) => {
              const s = workerTaskStatus[tid] ?? "idle";
              const title = workerTaskTitles[tid];
              const display = title
                ? `${t("perTask.agent.worker")} · ${tid}: ${title}`
                : `${t("perTask.agent.worker")} · ${tid}`;
              // Per-task worker cards still share the same
              // `worker` AgentLivePanel — the panel shows the
              // same role-wide timeline because the runtime
              // emits a single `worker` bucket. Per-task
              // event-level timeline is in the right panel.
              return (
                <AgentLivePanelTooltip key={tid} role="worker">
                  <AgentCard
                    role="worker"
                    name={display}
                    status={s}
                    statusLabel={t(`agentCard.status.${s}`)}
                    subtitle={t("leftRoster.workerSubtitle")}
                  />
                </AgentLivePanelTooltip>
              );
            })
          ) : (
            // No per-task events yet — fall back to the
            // legacy single worker card so phases before
            // dispatch still get a status row.
            <AgentLivePanelTooltip role="worker">
              <AgentCard
                role="worker"
                name={t("perTask.agent.worker")}
                status={agentStatus.worker.status}
                statusLabel={t(`agentCard.status.${agentStatus.worker.status}`)}
                subtitle={t("leftRoster.workerSubtitle")}
              />
            </AgentLivePanelTooltip>
          )}
        </div>
      )}

      {tab === "files" && (
        <div className="mt-2">
          <FileTree pollMs={5000} />
        </div>
      )}
    </div>
  );
}

// v0.4.29 (Phase B): unused helper retained as a re-export so
// any future consumer (e.g. AgentLivePanel) can map an event
// agent_id back to its dashboard head role. The roster no
// longer needs it itself.
export { agentIdToHeadRole };
