/**
 * SearchBugPanel — Settings > About > "Search my bug" sub-panel.
 *
 * The chairman's Polish 15 use case: the user gets an error
 * code on the ErrorBoundary screen (e.g. "FE-3a7b9c2d"), they
 * paste it into this panel, and we search the daily-rolling
 * log file for matching lines.
 *
 * The actual search runs via the `search_log` Tauri command
 * (added in Polish 15). It returns a JSON envelope
 * `{ matches: string[], scanned: number, truncated: bool }`.
 */
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { invoke } from '@tauri-apps/api/core';
import { tErr } from '../lib/errs.js';

interface SearchResult {
  matches: string[];
  scanned: number;
  truncated: boolean;
}

export function SearchBugPanel() {
  const { t } = useTranslation();
  const [code, setCode] = useState('');
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<SearchResult | null>(null);
  const [err, setErr] = useState<string | null>(null);
  // BUG-010 fix (event 000023): panic dumps are the most useful
  // thing to find when debugging a crash — but the previous code
  // hard-excluded them. Now the user can opt in via a checkbox;
  // by default they're still excluded because panic dumps can
  // be hundreds of KB of backtrace (false-positive heavily on
  // substring searches), but the user knows best when they're
  // hunting a crash.
  const [includePanicLogs, setIncludePanicLogs] = useState(false);

  const search = async () => {
    const trimmed = code.trim();
    if (!trimmed) return;
    setBusy(true);
    setErr(null);
    setResult(null);
    try {
      const r = await invoke<SearchResult>('search_log', {
        code: trimmed,
        since: null,
        include_panic_logs: includePanicLogs,
      });
      setResult(r);
    } catch (e) {
      setErr(tErr(t, e, 'settings.about.searchBugError'));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="mt-3 rounded-md border border-border bg-surface-2 p-3">
      <label className="mb-1 block text-[10px] font-semibold uppercase tracking-wide text-text-secondary">
        🔍 {t('settings.about.searchBug')}
      </label>
      <p className="mb-2 text-[10px] text-text-secondary">
        {t('settings.about.searchBugHint')}
      </p>
      <div className="flex gap-2">
        <input
          type="text"
          value={code}
          // BUG-056 fix (event 000024): apply uppercase + trim
          // only on submit (and on the disabled check), not on
          // every keystroke. The previous code did
          // `e.target.value.toUpperCase().trim()` per keystroke,
          // which truncated intermediate whitespace and made
          // typing feel janky (cursor jumped when the trimmed
          // value was shorter than the raw value).
          onChange={(e) => setCode(e.target.value.toUpperCase())}
          onKeyDown={(e) => e.key === 'Enter' && void search()}
          placeholder={t('settings.about.searchBugPlaceholder')}
          className="flex-1 rounded border border-border bg-surface-1 px-2 py-1.5 font-mono text-xs outline-none focus:border-chief"
          aria-label={t('settings.about.searchBugPlaceholder')}
        />
        <button
          type="button"
          onClick={() => void search()}
          disabled={busy || !code.trim()}
          className="rounded border border-chief/40 bg-chief/10 px-3 py-1.5 text-[11px] text-chief hover:bg-chief/20 disabled:opacity-50"
        >
          {busy ? t('settings.about.searchBugSearching') : t('settings.about.searchBugButton')}
        </button>
      </div>
      <label className="mt-2 flex items-center gap-1.5 text-[10px] text-text-secondary">
        <input
          type="checkbox"
          checked={includePanicLogs}
          onChange={(e) => setIncludePanicLogs(e.target.checked)}
          className="h-3 w-3"
        />
        {t('settings.about.includePanicLogs')}
      </label>

      {err && (
        <p
          role="alert"
          aria-live="polite"
          className="mt-2 text-[10px] text-status-failed"
        >
          {err}
        </p>
      )}

      {result !== null && (
        <div className="mt-2">
          <p className="text-[10px] text-text-secondary">
            {t('settings.about.searchBugScanned', {
              count: result.scanned,
              matches: result.matches.length,
            })}
            {result.truncated && ' (truncated to 200)'}
          </p>
          {result.matches.length === 0 ? (
            <p className="mt-1 text-[10px] text-text-secondary">
              {t('settings.about.searchBugEmpty')}
            </p>
          ) : (
            <pre className="mt-1 max-h-48 overflow-y-auto rounded bg-surface-1 p-2 font-mono text-[10px] leading-relaxed">
              {result.matches.join('\n')}
            </pre>
          )}
        </div>
      )}
    </div>
  );
}
