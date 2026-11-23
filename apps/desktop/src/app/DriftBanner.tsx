import { useTranslation } from "react-i18next";

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
export interface DriftBannerProps {
  sidecar: string;
  minCompatible: string;
  onDismiss: () => void;
}

export function DriftBanner({ sidecar, minCompatible, onDismiss }: DriftBannerProps) {
  const { t } = useTranslation();
  return (
    <div
      role="alert"
      className="flex items-center justify-between gap-4 border-b border-error bg-error/15 px-4 py-2 text-xs text-primary"
    >
      <span>{t("drift.message", { sidecar, expected: minCompatible })}</span>
      <button
        type="button"
        onClick={onDismiss}
        className="rounded-md border border-error/40 px-2 py-0.5 text-xs text-error hover:bg-error/25"
      >
        {t("drift.dismiss")}
      </button>
    </div>
  );
}
