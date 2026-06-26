import { Component, type ErrorInfo, type ReactNode } from 'react';
import i18n from '../i18n/index.js';
import { invoke } from '@tauri-apps/api/core';
import { open as openExternal } from '@tauri-apps/plugin-shell';

/**
 * Top-level React error boundary.
 *
 * Catches any uncaught exception thrown during render, in a
 * useEffect, or in a child component. Without this, a single
 * thrown error blanks the entire webview and the user has no
 * recourse except to kill the process — exactly the failure mode
 * that hurt Phase 0 testing.
 *
 * On error we render a "出错了 v0.4.0 · Build <sha>" screen with:
 *   - the error message (truncated)
 *   - the component stack
 *   - "复制日志" — copy the captured console buffer to clipboard
 *   - "重启应用" — reload the webview (clears in-memory state)
 *   - "上报问题" — open a pre-filled GitHub issue URL with the
 *     error message and version
 *
 * The error is also written to the Rust log file via the
 * `log_frontend_error` Tauri command (added in Phase 2.3), so a
 * user can attach the persisted log in a follow-up bug report.
 */

interface ErrorBoundaryProps {
  children: ReactNode;
  /** App version (passed in so the error screen can show it). */
  appVersion: string;
  /** Build SHA or tag for the error screen. */
  buildSha?: string;
}

interface ErrorBoundaryState {
  error: Error | null;
  componentStack: string | null;
  /** Stable 8-char hex code derived from FNV-1a hash of the
   *  error message + first line of stack. Used as a unique
   *  identifier the user can paste into a bug report. */
  errorCode: string | null;
  /** Rolling buffer of console.error / console.warn captured
   *  between mount and the current error. Capped at 50 entries. */
  capturedLogs: string[];
}

const MAX_CAPTURED_LOGS = 50;


// ── FNV-1a 32-bit hash (no dep, just inline) ───────────────────
function fnv1a(s: string): string {
  let h = 0x811c9dc5 >>> 0;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return ('00000000' + h.toString(16)).slice(-8);
}



export class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  override state: ErrorBoundaryState = {
    error: null,
    componentStack: null,
    errorCode: null,
    capturedLogs: [],
  };

  private originalConsoleError: typeof console.error | null = null;
  private originalConsoleWarn: typeof console.warn | null = null;

  override componentDidMount(): void {
    // Monkey-patch console.error and console.warn to capture
    // everything we see leading up to the crash. We restore them
    // on unmount so other ErrorBoundary instances don't stack
    // wrappers.
    this.originalConsoleError = console.error;
    this.originalConsoleWarn = console.warn;
    console.error = (...args: unknown[]) => {
      this.appendLog('error', args);
      this.originalConsoleError?.(...args);
    };
    console.warn = (...args: unknown[]) => {
      this.appendLog('warn', args);
      this.originalConsoleWarn?.(...args);
    };
  }

  override componentWillUnmount(): void {
    if (this.originalConsoleError) console.error = this.originalConsoleError;
    if (this.originalConsoleWarn) console.warn = this.originalConsoleWarn;
  }

  static getDerivedStateFromError(error: Error): Partial<ErrorBoundaryState> {
    return { error };
  }

  override componentDidCatch(error: Error, info: ErrorInfo): void {
    // Best-effort: forward to the Rust log file so the user can
    // grab it from %APPDATA%/flowntier/logs/flowntier.log.YYYY-MM-DD
    // (added in Phase 2.3). We don't await — the boundary
    // shouldn't block the error UI.
    void invoke('log_frontend_error', {
      message: error.message,
      stack: error.stack ?? '',
      componentStack: info.componentStack ?? '',
    }).catch((e) => {
      // The Rust side might not be up yet on first-launch crashes;
      // that's fine, we already have it in capturedLogs.
      console.warn('[ErrorBoundary] log_frontend_error failed:', e);
    });
    this.setState({
      componentStack: info.componentStack ?? null,
      errorCode: 'FE-' + fnv1a((error.message ?? '') + '|' + (error.stack?.split('\n')[0] ?? '')),
    });
  }

  private appendLog(level: 'error' | 'warn', args: unknown[]): void {
    const line = `[${new Date().toISOString()}] ${level}: ${args
      .map((a) => (typeof a === 'string' ? a : JSON.stringify(a)))
      .join(' ')}`;
    this.setState((prev) => ({
      capturedLogs: [...prev.capturedLogs, line].slice(-MAX_CAPTURED_LOGS),
    }));
  }

  private copyLogs = async (): Promise<void> => {
    const payload = [
      `Flowntier ${this.props.appVersion}${this.props.buildSha ? ` (build ${this.props.buildSha})` : ''}`,
      `Captured at: ${new Date().toISOString()}`,
      '',
      '=== Captured console output ===',
      ...this.state.capturedLogs,
      '',
      '=== React error ===',
      `Message: ${this.state.error?.message ?? '(unknown)'}`,
      `Stack:`,
      this.state.error?.stack ?? '(no stack)',
      '',
      '=== Component stack ===',
      this.state.componentStack ?? '(none)',
    ].join('\n');
    try {
      await navigator.clipboard.writeText(payload);
      alert('日志已复制到剪贴板');
    } catch (e) {
      // Fallback: select-and-prompt for environments without clipboard API
      console.warn('clipboard.writeText failed:', e);
      prompt('复制以下日志:', payload);
    }
  };

  private restart = (): void => {
    // window.location.reload re-mounts the React tree, which is
    // sufficient for most crashes. We deliberately do NOT use
    // @tauri-apps/api/window's .close() because that kills the
    // process and the user would have to re-launch by hand.
    window.location.reload();
  };

  private report = async (): Promise<void> => {
    const err = this.state.error;
    const params = new URLSearchParams({
      labels: 'bug',
      title: `[${this.state.errorCode ?? 'FE-????'}] v${this.props.appVersion}: ${(err?.message ?? 'crash').slice(0, 60)}`,
      body: [
        '**Error code**: `' + (this.state.errorCode ?? 'FE-????') + '`',
        '',
        '_(describe what you were doing)_',
        '',
        '**Version**',
        '',
        `- Flowntier ${this.props.appVersion}`,
        ...(this.props.buildSha ? [`- Build ${this.props.buildSha}`] : []),
        '',
        '**Error message**',
        '',
        '```',
        err?.message ?? '(unknown)',
        '```',
        '',
        '**Stack**',
        '',
        '```',
        (err?.stack ?? '(no stack)').slice(0, 1500),
        '```',
        '',
        '**Logs**',
        '',
        '_Use the "复制日志" button on the error screen, then paste here._',
      ].join('\n'),
    });
    const url = `https://github.com/Thatgfsj/Flowntier/issues/new?${params.toString()}`;
    try {
      await openExternal(url);
    } catch (e) {
      // No shell capability (shouldn't happen — capabilities/default.json
      // grants shell:allow-open) — fall back to clipboard so the user
      // can paste manually.
      console.warn('openExternal failed:', e);
      prompt('复制以下 URL 到浏览器打开:', url);
    }
  };

  override render(): ReactNode {
    if (!this.state.error) return this.props.children;

    const { appVersion, buildSha } = this.props;
    const message = this.state.error.message ?? '(unknown)';
    // t() is bound at render time; safe in class components because
    // we re-read i18n.language on every render (which happens when
    // i18n emits a 'languageChanged' event and React re-renders).
    const t = i18n.t.bind(i18n);
    return (
      <div className="flex h-screen w-screen flex-col items-center justify-center bg-surface-1 px-6 text-text-primary">
        <div className="max-w-2xl space-y-6">
          <div>
            <h1 className="text-3xl font-semibold text-error">
              {t('error.title', { version: appVersion })}
              {buildSha && (
                <span className="ml-2 text-sm font-normal text-text-secondary">
                  · Build {buildSha}
                </span>
              )}
              {this.state.errorCode && (
                <code
                  aria-label="Error code (paste this into bug reports)"
                  className="ml-2 select-all whitespace-nowrap rounded border border-error/40 bg-error/10 px-2 py-0.5 font-mono text-sm font-normal text-error"
                >
                  {this.state.errorCode}
                </code>
              )}
            </h1>
            <p className="mt-2 text-sm text-text-secondary">
              {t('error.subtitle')}
            </p>
          </div>

          <div className="rounded-md border border-border bg-surface-2 p-4 font-mono text-xs">
            <div className="text-text-secondary">{t('error.message')}</div>
            <div className="mt-1 break-words text-error">{message}</div>
            {this.state.componentStack && (
              <>
                <div className="mt-3 text-text-secondary">{t('error.componentStack')}</div>
                <pre className="mt-1 max-h-40 overflow-auto whitespace-pre-wrap break-words text-text-secondary">
                  {this.state.componentStack}
                </pre>
              </>
            )}
          </div>

          <div className="flex flex-wrap gap-3">
            <button
              type="button"
              onClick={() => void this.copyLogs()}
              className="rounded-md border border-border bg-surface-2 px-4 py-2 text-sm hover:bg-surface-3 focus:outline-none focus:ring-2 focus:ring-accent/50"
            >
              {t('error.action.copyLogs')}
            </button>
            <button
              type="button"
              onClick={this.restart}
              className="rounded-md border border-border bg-surface-2 px-4 py-2 text-sm hover:bg-surface-3 focus:outline-none focus:ring-2 focus:ring-accent/50"
            >
              {t('error.action.restart')}
            </button>
            <button
              type="button"
              onClick={() => void this.report()}
              className="rounded-md border border-accent bg-accent/10 px-4 py-2 text-sm text-accent hover:bg-accent/20 focus:outline-none focus:ring-2 focus:ring-accent/50"
            >
              {t('error.action.report')}
            </button>
          </div>

          <p className="text-xs text-text-secondary">
            {t('error.logLocation', {
              path: '%APPDATA%/flowntier/logs/',
            })}
          </p>
        </div>
      </div>
    );
  }
}