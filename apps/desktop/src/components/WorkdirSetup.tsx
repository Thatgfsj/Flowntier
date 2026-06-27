/**
 * WorkdirSetup — first-launch workdir picker.
 *
 * The chairman said: '第一次打开Flowntier的时候让用户设置工作目录，
 * 然后按任务让AI自己选择创建项目级别的目录（在工作目录下）'.
 *
 * Flow:
 *  1. On first launch (or whenever no workdir is configured), the
 *     shell shows this full-screen dialog.
 *  2. The user picks a directory via the OS file picker
 *     (@tauri-apps/plugin-dialog's open({ directory: true })).
 *     The picked path is the "workspace" — the parent of all
 *     future project directories.
 *  3. The user can also type the path manually.
 *  4. On confirm, we persist the path via a new
 *     `set_workdir` Tauri command (Rust side writes to a small
 *     config file or to the kv table).
 *  5. Subsequent launches: the dialog is skipped. The user can
 *     change the workdir later from Settings → About.
 *
 * Note: this is a full-screen takeover. It must be dismissed
 * before the main dashboard renders (handled by App.tsx via a
 *     workdir === null gate).
 */
import { useState } from 'react';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { useTranslation } from 'react-i18next';

export interface WorkdirSetupProps {
  /**
   * Optional pre-filled path (e.g. when re-opening from Settings
   * the user wants to see their current workdir).
   */
  initialPath?: string;
  /**
   * Called when the user confirms a workdir. Receives the
   * absolute path. The parent should persist + re-render.
   * May be async (returns a Promise) — e.g. App.tsx awaits
   * `invoke('set_workdir_with_nwt')` before resolving.
   */
  // BUG-055 fix (event 000024): allow async — the prop was
  // typed as `(path: string) => void` but App.tsx passes
  // `(path) => Promise<void>`. TypeScript was inferring the
  // broader signature and skipping the mismatch; tightening
  // it now catches accidental sync-only callbacks.
  onConfirm: (path: string) => void | Promise<void>;
  /**
   * Called when the user clicks "Skip" (advanced users who want
   * to set the workdir later from the command line). Most users
   * should NOT skip — without a workdir, the AI can't create
   * project sub-directories.
   */
  onSkip?: () => void;
  /**
   * Mode: 'first-launch' (full-screen, mandatory) vs
   * 'settings' (modal in Settings modal, optional). Affects copy
   * + whether Skip is shown.
   */
  mode: 'first-launch' | 'settings';
}

export function WorkdirSetup({ initialPath, onConfirm, onSkip, mode }: WorkdirSetupProps) {
  const { t } = useTranslation();
  const [path, setPath] = useState(initialPath ?? '');
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const isFirstLaunch = mode === 'first-launch';

  const pick = async () => {
    setBusy(true);
    setErr(null);
    try {
      const picked = await openDialog({
        directory: true,
        multiple: false,
        title: t('workdir.pickTitle'),
      });
      if (typeof picked === 'string' && picked.length > 0) {
        setPath(picked);
      }
    } catch (e) {
      setErr(tErr(t, e, 'workdir.errorPick'));
    } finally {
      setBusy(false);
    }
  };

  const confirm = () => {
    const trimmed = path.trim();
    if (trimmed.length === 0) {
      setErr(t('workdir.errorEmpty'));
      return;
    }
    setErr(null);
    onConfirm(trimmed);
  };

  return (
    <div
      className={
        isFirstLaunch
          ? 'flex h-screen w-screen items-center justify-center bg-surface-1 px-6'
          : 'flex flex-col gap-3'
      }
    >
      <div className="w-full max-w-xl rounded-lg border border-border bg-surface-1 p-6 shadow-sm">
        <h1 className="text-xl font-semibold text-text-primary">
          {t('workdir.title')}
        </h1>
        <p className="mt-1 text-sm text-text-secondary">
          {isFirstLaunch
            ? t('workdir.subtitleFirst')
            : t('workdir.subtitleSettings')}
        </p>

        <div className="mt-4 flex gap-2">
          <input
            type="text"
            value={path}
            onChange={(e) => {
              setPath(e.target.value);
              setErr(null);
            }}
            placeholder={t('workdir.placeholder')}
            className="flex-1 rounded border border-border bg-surface-2 px-3 py-2 font-mono text-sm outline-none focus:border-chief"
            aria-label={t('workdir.placeholder')}
          />
          <button
            type="button"
            onClick={() => void pick()}
            disabled={busy}
            className="rounded border border-border bg-surface-2 px-3 py-2 text-sm text-text-secondary hover:bg-surface-3 hover:text-primary disabled:opacity-50"
          >
            📁 {t('workdir.browse')}
          </button>
        </div>

        {err && (
          <p
            role="alert"
            aria-live="polite"
            className="mt-2 text-xs text-status-failed"
          >
            {err}
          </p>
        )}

        {path.trim().length === 0 && (
          <p className="mt-3 text-xs text-text-secondary">
            {t('workdir.hint')}
          </p>
        )}

        <div className="mt-5 flex items-center justify-end gap-2">
          {isFirstLaunch && onSkip && (
            <button
              type="button"
              onClick={onSkip}
              className="text-xs text-text-secondary hover:text-primary"
            >
              {t('workdir.skip')}
            </button>
          )}
          <button
            type="button"
            onClick={confirm}
            disabled={path.trim().length === 0 || busy}
            className="rounded-md bg-chief px-4 py-2 text-sm font-medium text-white hover:bg-chief/90 disabled:opacity-50"
          >
            {isFirstLaunch ? t('workdir.confirmFirst') : t('workdir.confirmSettings')}
          </button>
        </div>
      </div>
    </div>
  );
}

// Avoid importing tErr from a separate file; mirror its inline
// implementation here so the workdir dialog is self-contained.
// eslint-disable-next-line @typescript-eslint/no-explicit-any
function tErr(t: (k: string, options?: any) => string, error: unknown, fallbackKey: string): string {
  const raw = error instanceof Error ? error.message : String(error);
  return t(fallbackKey, { error: raw });
}
