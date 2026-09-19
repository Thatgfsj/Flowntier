/**
 * CheckpointsStrip — v0.4.36 compact always-visible strip.
 *
 * Lives DIRECTLY above <BottomConsole /> in App.tsx. It must be
 * mounted OUTSIDE BottomConsole because BottomConsole collapses
 * itself to null when it has no console events (`hasContent`
 * gate) — a strip nested inside it would vanish exactly when a
 * fresh workspace has no logs yet, and checkpoints are precisely
 * the thing you want to see early (they're the undo path for
 * everything the agents just did).
 *
 * Features per spec:
 *   - listCheckpoints() on mount + a "⟳" refresh button.
 *   - "+ 检查点" creates a manual checkpoint then refreshes.
 *   - Each row: label / id / kind / time + a "恢复" button.
 *   - Restore is two-phase: confirm=false → preview → the user
 *     sees a small panel (first 10 files + total count) and must
 *     click 确认恢复 before confirm=true goes out.
 *   - {ok:false} results render as red text; empty list shows a
 *     "暂无检查点" row instead of hiding the strip.
 */
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  createCheckpoint,
  listCheckpoints,
  restoreCheckpoint,
  type CheckpointInfo,
  type RestorePreview,
} from "../lib/api.js";
import { summarizeRestoreFiles, formatCheckpointTime } from "./checkpointUtil.js";

/** One pending restore confirmation. */
interface PendingRestore {
  checkpoint: CheckpointInfo;
  preview: RestorePreview;
}

export function CheckpointsStrip() {
  const { t } = useTranslation();
  const [rows, setRows] = useState<CheckpointInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);
  const [pending, setPending] = useState<PendingRestore | null>(null);
  const [restoring, setRestoring] = useState(false);
  const [restoreError, setRestoreError] = useState<string | null>(null);
  const [restoredCount, setRestoredCount] = useState<number | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const resp = await listCheckpoints();
      setRows(Array.isArray(resp?.rows) ? resp.rows : []);
      setLoadError(null);
    } catch (e) {
      setLoadError(t("checkpoints.loadFailed", { error: String(e) }));
    } finally {
      setLoading(false);
    }
  }, [t]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const handleCreate = useCallback(async () => {
    if (creating) return;
    setCreating(true);
    setCreateError(null);
    try {
      const r = await createCheckpoint("manual");
      if (!r.ok) {
        setCreateError(r.error ?? t("checkpoints.createFailed", { error: "-" }));
      }
    } catch (e) {
      setCreateError(t("checkpoints.createFailed", { error: String(e) }));
    } finally {
      setCreating(false);
      void refresh();
    }
  }, [creating, refresh, t]);

  // Phase 1: ask for the preview (confirm:false). Never writes.
  const askRestore = useCallback(
    async (cp: CheckpointInfo) => {
      setRestoreError(null);
      setRestoredCount(null);
      try {
        const r = await restoreCheckpoint({ id: cp.id, confirm: false });
        if (r.ok) {
          // applied:false branch carries the preview.
          if (!r.applied && r.preview) {
            setPending({ checkpoint: cp, preview: r.preview });
          }
        } else {
          setRestoreError(t("checkpoints.restoreFailed", { error: r.error }));
        }
      } catch (e) {
        setRestoreError(t("checkpoints.restoreFailed", { error: String(e) }));
      }
    },
    [t],
  );

  // Phase 2: the chairman confirmed — apply.
  const confirmRestore = useCallback(async () => {
    if (!pending || restoring) return;
    setRestoring(true);
    setRestoreError(null);
    try {
      const r = await restoreCheckpoint({ id: pending.checkpoint.id, confirm: true });
      if (r.ok) {
        setRestoredCount(r.applied ? r.files_restored : 0);
        setPending(null);
        void refresh();
      } else {
        setRestoreError(t("checkpoints.restoreFailed", { error: r.error }));
      }
    } catch (e) {
      setRestoreError(t("checkpoints.restoreFailed", { error: String(e) }));
    } finally {
      setRestoring(false);
    }
  }, [pending, refresh, restoring, t]);

  const cancelRestore = useCallback(() => {
    setPending(null);
  }, []);

  const summary = pending ? summarizeRestoreFiles(pending.preview.files, 10) : null;

  return (
    <section
      className="relative shrink-0 border-t border-border bg-surface-2 px-3 py-1.5"
      aria-label={t("checkpoints.aria")}
    >
      {/* Confirm panel — floats above the strip rows */}
      {pending && summary && (
        <div className="absolute bottom-full left-3 right-3 z-10 mb-1 rounded border border-chief/40 bg-surface-1 p-2 shadow-lg">
          <div className="mb-1 text-[11px] font-semibold text-text-primary">
            {t("checkpoints.confirmTitle", { label: pending.checkpoint.label })}
          </div>
          <div className="mb-1 text-[11px] text-text-secondary">
            {t("checkpoints.affectedFiles", { count: summary.total })}
            {pending.preview.approximate && (
              <span className="ml-1 italic">{t("checkpoints.approximate")}</span>
            )}
          </div>
          {summary.shown.length > 0 && (
            <ul className="mb-1 max-h-28 overflow-y-auto font-mono text-[10px] text-text-secondary">
              {summary.shown.map((f) => (
                <li key={f} className="truncate" title={f}>
                  ▸ {f}
                </li>
              ))}
            </ul>
          )}
          {summary.truncated && (
            <div className="mb-1 text-[10px] italic text-text-secondary">
              {t("checkpoints.moreFiles", { hidden: summary.hidden, total: summary.total })}
            </div>
          )}
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={() => {
                void confirmRestore();
              }}
              disabled={restoring}
              className="rounded border border-status-failed/50 bg-status-failed/10 px-2 py-0.5 text-[11px] text-status-failed hover:bg-status-failed/20 disabled:opacity-50"
            >
              {restoring ? t("checkpoints.restoring") : t("checkpoints.confirm")}
            </button>
            <button
              type="button"
              onClick={cancelRestore}
              disabled={restoring}
              className="rounded border border-border bg-surface-2 px-2 py-0.5 text-[11px] text-text-secondary hover:bg-surface-3 disabled:opacity-50"
            >
              {t("checkpoints.cancel")}
            </button>
          </div>
        </div>
      )}

      <div className="flex items-center gap-2">
        <h2 className="shrink-0 text-[11px] font-semibold uppercase tracking-wide text-text-secondary">
          {t("checkpoints.title")}
        </h2>
        <button
          type="button"
          onClick={() => {
            void refresh();
          }}
          className="shrink-0 rounded border border-border bg-surface-1 px-1.5 py-0.5 text-[10px] text-text-secondary hover:bg-surface-3"
          title={t("checkpoints.refreshTitle")}
          aria-label={t("checkpoints.refreshTitle")}
        >
          ⟳
        </button>
        <button
          type="button"
          onClick={() => {
            void handleCreate();
          }}
          disabled={creating}
          className="shrink-0 rounded border border-chief/40 bg-chief/10 px-2 py-0.5 text-[10px] text-chief hover:bg-chief/20 disabled:opacity-50"
        >
          {creating ? t("checkpoints.creating") : t("checkpoints.create")}
        </button>

        {loading ? (
          <span className="min-w-0 truncate text-[11px] text-text-secondary">
            {t("checkpoints.loading")}
          </span>
        ) : rows.length === 0 ? (
          <span className="min-w-0 truncate text-[11px] text-text-secondary">
            {t("checkpoints.empty")}
          </span>
        ) : (
          <ul className="flex min-w-0 flex-1 items-center gap-2 overflow-x-auto">
            {rows.map((cp) => (
              <li
                key={cp.id}
                className="flex shrink-0 items-center gap-1.5 rounded border border-border bg-surface-1 px-2 py-0.5 text-[10px]"
                title={`${cp.id} · ${cp.kind}`}
              >
                <span className="font-mono text-text-primary">{cp.label}</span>
                <span className="font-mono text-text-tertiary">#{cp.id.slice(0, 8)}</span>
                <span className="text-text-secondary">{cp.kind}</span>
                <span className="text-text-secondary">{formatCheckpointTime(cp.at)}</span>
                <button
                  type="button"
                  onClick={() => {
                    void askRestore(cp);
                  }}
                  disabled={restoring}
                  className="rounded border border-border bg-surface-2 px-1.5 py-0.5 text-[10px] text-text-secondary hover:bg-surface-3 hover:text-text-primary disabled:opacity-50"
                >
                  {t("checkpoints.restore")}
                </button>
              </li>
            ))}
          </ul>
        )}

        {restoredCount !== null && (
          <span className="ml-auto shrink-0 text-[11px] text-status-done">
            {t("checkpoints.restored", { count: restoredCount })}
          </span>
        )}
      </div>

      {/* Errors render as red text per spec */}
      {(loadError || createError || restoreError) && (
        <div className="mt-1 text-[11px] text-status-failed">
          {restoreError ?? createError ?? loadError}
        </div>
      )}
    </section>
  );
}
