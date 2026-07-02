/**
 * ErrorBadge — v0.4.21 (event 000066).
 *
 * Red-dot badge in the TopBar that surfaces the most-recent
 * pipe-server error records. Polls /api/errors/recent every
 * 10s; clicking the badge opens a small dropdown listing the
 * last 10 errors with severity + source + summary + detail.
 *
 * Why this exists: chairman's directive "日志弄详细一点" — we
 * need a UI affordance so transient errors (workspace swap
 * rejects, run_task timeouts, quota failures) aren't lost in
 * the stderr/tracing log file.
 */
import { useEffect, useState } from 'react';
import { getRecentErrors, type ErrorRecord } from '../lib/api.js';

export function ErrorBadge() {
  const [rows, setRows] = useState<ErrorRecord[]>([]);
  const [open, setOpen] = useState(false);

  useEffect(() => {
    let cancelled = false;
    const tick = async () => {
      try {
        const r = await getRecentErrors(10);
        if (!cancelled) setRows(r.rows);
      } catch {
        // Silent — badge just shows zero.
      }
    };
    void tick();
    const t = setInterval(tick, 10_000);
    return () => { cancelled = true; clearInterval(t); };
  }, []);

  const errorCount = rows.filter((r) => r.severity === 'error').length;
  const warnCount = rows.filter((r) => r.severity === 'warn').length;

  return (
    <div className="relative">
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        aria-label="Recent errors"
        aria-expanded={open}
        title={rows.length === 0 ? 'no recent errors' : `${errorCount} error(s), ${warnCount} warning(s)`}
        className="relative flex items-center gap-1 rounded-md border border-border bg-surface-1 px-2 py-1 text-xs transition-colors hover:text-primary"
      >
        <span className="text-base leading-none">⚠️</span>
        {rows.length > 0 && (
          <span
            className={
              'ml-0.5 inline-flex h-4 min-w-[1rem] items-center justify-center rounded-full px-1 text-[10px] font-semibold ' +
              (errorCount > 0
                ? 'bg-red-600 text-white'
                : 'bg-yellow-500 text-black')
            }
          >
            {rows.length}
          </span>
        )}
      </button>
      {open && (
        <div
          role="dialog"
          aria-label="Recent errors"
          className="absolute right-0 top-full z-50 mt-1 w-96 max-w-[90vw] rounded-md border border-border bg-surface-1 p-2 shadow-xl"
        >
          <header className="mb-2 flex items-center justify-between">
            <h3 className="text-xs font-semibold">最近错误</h3>
            <button
              type="button"
              onClick={() => setOpen(false)}
              className="rounded px-2 py-0.5 text-[10px] hover:bg-surface-3"
            >
              关闭
            </button>
          </header>
          {rows.length === 0 && (
            <div className="px-2 py-1 text-[11px] text-text-secondary">
              暂无错误
            </div>
          )}
          <ul className="max-h-[60vh] space-y-1 overflow-y-auto">
            {rows.map((r, i) => (
              <li
                key={`${r.at}-${i}`}
                className={
                  'rounded border px-2 py-1 text-[11px] ' +
                  (r.severity === 'error'
                    ? 'border-red-500/50 bg-red-900/20 text-red-100'
                    : r.severity === 'warn'
                      ? 'border-yellow-500/50 bg-yellow-900/20 text-yellow-100'
                      : 'border-border bg-surface-2 text-text-primary')
                }
              >
                <div className="flex items-center justify-between gap-2">
                  <span className="font-mono text-[10px] text-text-secondary">
                    {new Date(r.at * 1000).toLocaleTimeString()}
                  </span>
                  <span className="rounded bg-surface-3 px-1 py-0.5 font-mono text-[10px]">
                    {r.source}
                  </span>
                </div>
                <div className="mt-0.5">{r.summary}</div>
                {r.detail && (
                  <div className="mt-0.5 text-[10px] text-text-secondary">
                    {r.detail}
                  </div>
                )}
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}