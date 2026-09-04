import type { FormEvent } from "react";
import { useTranslation } from "react-i18next";

export interface CommandDockProps {
  commandInput: string;
  onCommandChange: (value: string) => void;
  onCommandSubmit: () => void;
  /** Whether the runtime is currently running. */
  busy?: boolean;
  /** Optional reset button label (shown when workflow is complete). */
  resetLabel?: string;
  /** Recent commands for the autocomplete history dropdown. */
  recent?: string[];
}

/**
 * Z5 — bottom command dock. Where the user types requests to the Chief.
 * Sits above the console, at the very bottom of the window.
 */
export function CommandDock({
  commandInput,
  onCommandChange,
  onCommandSubmit,
  busy = false,
  resetLabel,
  recent,
}: CommandDockProps) {
  const { t } = useTranslation();
  const handleSubmit = (e: FormEvent) => {
    e.preventDefault();
    onCommandSubmit();
  };

  const isReset = resetLabel !== undefined && resetLabel.length > 0;
  const canSubmit = isReset || (!busy && commandInput.trim().length > 0);

  return (
    <form
      onSubmit={handleSubmit}
      className="relative flex shrink-0 items-center gap-3 border-t border-white/8 bg-surface-1/95 px-4 py-3 backdrop-blur-xl"
      aria-label={t("commandDock.placeholder")}
    >
      {/* Modern Inner Input Capsule */}
      <div className="flex flex-1 items-center gap-2.5 rounded-xl border border-white/10 bg-surface-2/90 px-3.5 py-2 shadow-sm transition-all focus-within:border-chief/60 focus-within:ring-2 focus-within:ring-chief/25">
        <span className="inline-flex shrink-0 items-center gap-1 rounded-md bg-chief/15 px-2 py-0.5 text-xs font-semibold text-chief">
          主理 ▸
        </span>
        <input
          type="text"
          value={commandInput}
          onChange={(e) => onCommandChange(e.target.value)}
          list={recent && recent.length > 0 ? "flowntier-cmd-history" : undefined}
          placeholder={isReset ? t("commandDock.empty") : t("commandDock.placeholder")}
          disabled={busy}
          className="flex-1 bg-transparent text-sm text-text-primary placeholder:text-text-tertiary focus:outline-none disabled:opacity-50"
          aria-label={t("commandDock.placeholder")}
        />
        <span className="hidden sm:inline-block rounded border border-white/10 bg-surface-3/60 px-1.5 py-0.5 text-[10px] font-mono text-text-tertiary">
          ↵ Enter
        </span>
      </div>

      <button
        type="submit"
        disabled={!canSubmit}
        className="flex shrink-0 items-center gap-1.5 rounded-xl bg-gradient-to-r from-chief to-blue-500 px-4 py-2.5 text-xs font-semibold text-white shadow-md shadow-chief/20 transition-all hover:brightness-110 active:scale-95 disabled:pointer-events-none disabled:opacity-40"
      >
        {isReset ? (
          resetLabel
        ) : (
          <>
            <span>{t("commandDock.submit")}</span>
            <svg className="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={2}
                d="M14 5l7 7m0 0l-7 7m7-7H3"
              />
            </svg>
          </>
        )}
      </button>

      {recent && recent.length > 0 && (
        <datalist id="flowntier-cmd-history">
          {recent.slice(0, 10).map((cmd, i) => (
            <option key={i} value={cmd} />
          ))}
        </datalist>
      )}
    </form>
  );
}
