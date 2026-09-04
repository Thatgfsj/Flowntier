/**
 * Z1 — top bar. Title + update banner + user menu.
 * The command input was moved to the bottom (CommandDock) in v0.2.
 *
 * v0.4.21 (event 000066): added the <ErrorBadge /> to the right
 * of the chat/settings buttons. Polls /api/errors/recent every
 * 10s and lights up red/yellow when the runtime emits any.
 * Chairman's directive: "日志弄详细一点" — this gives the
 * transient errors a UI affordance so they aren't lost.
 *
 * v0.4.22 (event 000118, fix 7): added a Stop button that's
 * only visible while a workflow is running. Clicking it shows
 * a confirmation modal; confirming fires onCancel which
 * delegates to App.tsx (which calls cancelWorkflow).
 */
import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { UpdateBanner } from "../lib/updater";
import { SUPPORTED } from "../i18n/index.js";
import { ErrorBadge } from "../components/ErrorBadge.js";

export interface TopBarProps {
  projectName: string;
  /** Optional subtitle for the project (e.g. current workflow). */
  subtitle?: string;
  /** Optional onClick for the settings button. */
  onSettingsClick?: () => void;
  /** Optional onClick for the chat-zone toggle. */
  onChatClick?: () => void;
  /** Whether the chat zone is currently expanded. */
  chatOpen?: boolean;
  /** Update banner state from the auto-update check (Phase 1.3). */
  updateBanner?: UpdateBanner;
  /** Click handler for the update banner. */
  onUpdateClick?: () => void;
  /**
   * v0.4.22 (event 000118, fix 7): click handler for the Stop
   * button. Only present when a workflow is running. App.tsx
   * implements this by calling `cancelWorkflow(currentWfId)`.
   */
  onCancel?: () => void | Promise<void>;
  /** True while we're waiting on cancelWorkflow() to resolve. */
  cancelling?: boolean;
}

export function TopBar({
  projectName,
  subtitle,
  onSettingsClick,
  onChatClick,
  chatOpen,
  updateBanner,
  onUpdateClick,
  onCancel,
  cancelling,
}: TopBarProps) {
  const { t } = useTranslation();
  const showUpdate = updateBanner?.available === true && typeof updateBanner.version === "string";
  // v0.4.22 (event 000118, fix 7): two-step confirmation.
  // First click opens the modal, second click (in the modal)
  // actually fires onCancel. Accidental clicks shouldn't kill
  // a 4-minute workflow.
  const [confirmOpen, setConfirmOpen] = useState(false);
  return (
    <header className="flex h-12 shrink-0 items-center gap-3 border-b border-white/8 bg-surface-1/95 px-4 backdrop-blur-md shadow-sm">
      <div className="flex items-center gap-2.5">
        <div className="flex h-6 w-6 items-center justify-center rounded-lg bg-gradient-to-br from-chief via-blue-500 to-indigo-600 text-xs font-bold text-white shadow-sm shadow-chief/30">
          F
        </div>
        <span className="font-semibold tracking-tight text-white">{projectName}</span>
        <span className="hidden sm:inline text-xs text-text-tertiary">{t("topbar.tagline")}</span>
      </div>
      {subtitle !== undefined && subtitle.length > 0 && (
        <span className="text-xs text-text-tertiary">/ {subtitle}</span>
      )}
      <div className="flex-1" />
      {showUpdate && (
        <button
          type="button"
          onClick={onUpdateClick}
          className="rounded-lg border border-accent/40 bg-accent/10 px-3 py-1.5 text-xs font-medium text-accent transition-all hover:bg-accent/20 focus:outline-none focus:ring-2 focus:ring-accent/50"
          title={t("update.tooltip")}
        >
          {t("update.available", { version: updateBanner!.version })}
        </button>
      )}
      {onCancel && (
        <>
          <button
            type="button"
            onClick={() => setConfirmOpen(true)}
            disabled={!!cancelling}
            aria-label={t("topbar.stopAria", { defaultValue: "停止当前工作流" })}
            className="rounded-md border border-status-failed/60 bg-status-failed/10 px-3 py-1.5 text-xs font-medium text-status-failed transition-colors hover:bg-status-failed/20 focus:outline-none focus:ring-2 focus:ring-status-failed/50 disabled:opacity-50"
            title={t("topbar.stopHint", { defaultValue: "立即中断当前工作流" })}
          >
            {cancelling
              ? t("topbar.stopping", { defaultValue: "停止中…" })
              : t("topbar.stop", { defaultValue: "停止" })}
          </button>
          {confirmOpen && (
            // Modal — overlays the entire app. Uses the same
            // surface palette as the Settings modal so it feels
            // native.
            <div
              className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-sm"
              role="dialog"
              aria-modal="true"
              aria-labelledby="topbar-cancel-title"
            >
              <div className="w-80 max-w-[90vw] rounded-md border border-border bg-surface-1 p-4 shadow-xl">
                <h2 id="topbar-cancel-title" className="text-sm font-semibold text-primary">
                  {t("topbar.cancelTitle", { defaultValue: "中断当前工作流？" })}
                </h2>
                <p className="mt-2 text-xs leading-relaxed text-text-secondary">
                  {t("topbar.cancelBody", {
                    defaultValue: "正在跑的工作流会立刻停下 (1 秒内), 已经写好的文件会保留。",
                  })}
                </p>
                <div className="mt-4 flex justify-end gap-2">
                  <button
                    type="button"
                    onClick={() => setConfirmOpen(false)}
                    className="rounded border border-border bg-surface-2 px-3 py-1 text-xs text-text-secondary hover:bg-surface-3"
                  >
                    {t("topbar.cancelDismiss", { defaultValue: "继续运行" })}
                  </button>
                  <button
                    type="button"
                    onClick={async () => {
                      setConfirmOpen(false);
                      if (onCancel) await onCancel();
                    }}
                    className="rounded bg-status-failed px-3 py-1 text-xs font-semibold text-black hover:brightness-110"
                  >
                    {t("topbar.cancelConfirm", { defaultValue: "确认停止" })}
                  </button>
                </div>
              </div>
            </div>
          )}
        </>
      )}
      {onChatClick && (
        <button
          type="button"
          onClick={onChatClick}
          aria-pressed={chatOpen}
          className={`rounded-lg border px-3 py-1.5 text-xs font-medium transition-all focus:outline-none focus:ring-2 focus:ring-chief/50 ${
            chatOpen
              ? "border-chief/60 bg-chief/15 text-chief shadow-sm shadow-chief/20"
              : "border-white/10 bg-surface-2/80 text-text-secondary hover:border-white/20 hover:text-white"
          }`}
        >
          {t("topbar.chat")}
        </button>
      )}
      <ErrorBadge />
      <button
        type="button"
        onClick={onSettingsClick}
        className="rounded-lg border border-white/10 bg-surface-2/80 px-3 py-1.5 text-xs font-medium text-text-secondary transition-all hover:border-white/20 hover:text-white focus:outline-none focus:ring-2 focus:ring-chief/50"
      >
        {t("topbar.settings")}
      </button>
      <LanguageToggle />
    </header>
  );
}

/**
 * Tiny two-state language toggle. Cycles between zh-CN and en-US.
 * Persists to localStorage via i18n.on('languageChanged').
 */
function LanguageToggle() {
  const { i18n, t } = useTranslation();
  const current = i18n.language?.startsWith("en") ? "en-US" : "zh-CN";
  const next = current === "zh-CN" ? "en-US" : "zh-CN";
  return (
    <button
      type="button"
      onClick={() => {
        window.dispatchEvent(new CustomEvent("flowntier:close-modals"));
        void i18n.changeLanguage(next);
      }}
      title={t("lang.label") + ": " + (SUPPORTED.find((l) => l === next) ?? "")}
      className="rounded-lg border border-white/10 bg-surface-2/80 px-2.5 py-1.5 text-xs font-medium text-text-secondary transition-all hover:border-white/20 hover:text-white focus:outline-none focus:ring-2 focus:ring-chief/50"
      aria-label={`Language: ${current}`}
    >
      🌐 {current === "zh-CN" ? t("lang.zh-CN") : t("lang.en-US")}
    </button>
  );
}
