/**
 * ChatZone — v0.4.19 minimal chat panel.
 *
 * Per chairman directive (event 000055): strip the in-ChatZone
 * provider/baseUrl/model/apiKeyEnv inputs. Those are now configured
 * in Settings → AI 供应商 + 角色 → 模型 分配. ChatZone sends
 * only `{ task, role }` and the pipe-server's `run_task` handler
 * resolves default_model + base_url + api_key from the role_overrides
 * table + the matching preset + the OS keystore.
 *
 * Visible affordances:
 *   - role picker (主理 / 实施 / 策划 / 审核 A / 审核 B / 汇报)
 *   - resolve status line (under role picker; shows "ok" / "no key"
 *     / "unconfigured" with the resolved model name and provider)
 *   - multiline task input
 *   - send button (also Ctrl/Cmd+Enter)
 *   - streaming assistant transcript
 *   - tool timeline (with command preview)
 *   - token usage + final status
 */
import { useCallback, useEffect, useMemo, useRef, useState, type FormEvent, type KeyboardEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { invoke } from '@tauri-apps/api/core';
import { useAgentStream } from '../hooks/useAgentStream.js';
import { getRoleResolveStatus, type RoleResolveStatus } from '../lib/api.js';

interface RoleSpec {
  id: string;
  /** i18n key suffix (e.g. "chief", "worker", "criticA") — the
   *  component resolves label + hint via t() at render time. */
  i18nKey: string;
}

/** BUG-FRONTEND-RT-4 (event 000030 follow-up): the role
 *  definitions used to be a hardcoded Chinese array. Now each
 *  entry carries an i18n key suffix; the consumer (useChatZone
 *  via buildRoles) translates both label and hint at render. */
const ROLE_DEFS: RoleSpec[] = [
  { id: 'agent:chief',    i18nKey: 'chief' },
  { id: 'agent:worker',   i18nKey: 'worker' },
  { id: 'agent:planner',  i18nKey: 'planner' },
  { id: 'agent:critic:a', i18nKey: 'criticA' },
  { id: 'agent:critic:b', i18nKey: 'criticB' },
  { id: 'agent:reporter', i18nKey: 'reporter' },
];

export interface ChatZoneProps {
  /** No props now — provider/model/api_key are server-resolved. */
}

export function ChatZone(_: ChatZoneProps = {}) {
  const { t } = useTranslation();
  const [task, setTask] = useState('');
  const [role, setRole] = useState<string>('agent:chief');
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [resolve, setResolve] = useState<RoleResolveStatus | null>(null);

  const { events, text, done, status, reset } = useAgentStream();
  const transcriptRef = useRef<HTMLDivElement>(null);

  const toolEvents = useMemo(
    () => events.filter((e) => e.kind === 'tool_started' || e.kind === 'tool_finished'),
    [events],
  );

  // Auto-scroll the transcript as text streams in. Disable when
  // the user has scrolled up to read history; re-engage when they
  // jump back to the bottom.
  useEffect(() => {
    const el = transcriptRef.current;
    if (!el) return;
    const nearBottom =
      el.scrollHeight - el.scrollTop - el.clientHeight < 64;
    if (nearBottom) {
      el.scrollTop = el.scrollHeight;
    }
  }, [text, events.length]);

  // v0.4.19: poll the role resolve status whenever the role
  // changes so the user can see "默认: minimax:MiniMax-Text-01" /
  // "未配置 API 密钥" / "未配置 default_model" inline.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const r = await getRoleResolveStatus(role);
        if (!cancelled) setResolve(r);
      } catch (e) {
        if (!cancelled) {
          setResolve({
            ok: false,
            error: typeof e === 'string' ? e : (e as Error)?.message ?? 'resolve failed',
          });
        }
      }
    })();
    return () => { cancelled = true; };
  }, [role]);

  const send = useCallback(async () => {
    const trimmed = task.trim();
    if (!trimmed || sending) return;
    setError(null);
    reset();
    setSending(true);
    try {
      // v0.4.19: send only { task, role } — server resolves the
      // rest from role_overrides + preset + OS keystore.
      const ok = await invoke<{ ok: boolean; error?: string; role?: string; hint?: string; status?: string }>(
        'run_agent_task',
        { body: { task: trimmed, role } },
      );
      if (!ok?.ok) {
        // The backend may return either 5xx-shaped envelope
        // (status: 503, ok:false, error) or a thrown Err (Promise
        // reject — caught by the catch below). Show the most
        // informative line available.
        const tail = ok?.error
          ? `${ok.error}${ok.hint ? ` — ${ok.hint}` : ''}`
          : ok?.status ?? '运行时未确认成功';
        setError(tail);
      }
    } catch (e) {
      // Async pipe failure (server not reachable, panic, etc.).
      const msg = typeof e === 'string' ? e : (e as Error).message;
      // 'HTTP 500: {...}' style — strip the JSON noise.
      const trimmed2 = msg?.replace(/^HTTP \d+:\s*/, '').slice(0, 240);
      setError(trimmed2 ?? 'unknown error');
    } finally {
      setSending(false);
    }
  }, [task, sending, reset, role]);

  const onSubmit = useCallback(
    (e: FormEvent) => {
      e.preventDefault();
      void send();
    },
    [send],
  );

  const onKeyDown = useCallback(
    (e: KeyboardEvent<HTMLTextAreaElement>) => {
      if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
        e.preventDefault();
        void send();
      }
    },
    [send],
  );

  return (
    <section
      className="flex h-full flex-col border-t border-border bg-surface-1"
      aria-label="ChatZone 跟角色对话"
    >
      {/* Header */}
      <header className="flex shrink-0 items-center justify-between gap-3 border-b border-border bg-surface-2 px-4 py-2">
        <div className="flex items-center gap-2">
          <span className="font-mono text-xs text-text-secondary">ChatZone ▸</span>
          <span className="text-sm text-text-secondary">{t('chatZone.subtitle', { defaultValue: '由设置中的角色分配配置驱动' })}</span>
        </div>
        <button
          type="button"
          onClick={reset}
          className="rounded border border-border px-2 py-0.5 text-xs text-text-secondary hover:bg-surface-1"
        >
          {t('chatZone.clear', { defaultValue: '清空' })}
        </button>
      </header>

      {/* Controls — only the role picker remains; everything else
          moved to Settings. */}
      <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-border bg-surface-2/60 px-4 py-2 text-xs">
        <label className="flex items-center gap-1">
          <span className="text-text-secondary">{t('chatZone.role', { defaultValue: '角色' })}</span>
          <select
            value={role}
            onChange={(e) => setRole(e.target.value)}
            disabled={sending}
            className="rounded border border-border bg-surface-1 px-2 py-1 disabled:opacity-50"
          >
            {ROLE_DEFS.map((r) => (
              <option key={r.id} value={r.id} title={t(`chatZone.roles.${r.i18nKey}Hint`)}>
                {t(`chatZone.roles.${r.i18nKey}`)}
              </option>
            ))}
          </select>
        </label>
        {/* Resolve status: shows what default_model + which preset +
           whether the API key is configured. Chairman previously had
           to type all of this manually; now it appears inline. */}
        {resolve && (
          <span
            className={
              resolve.ok
                ? 'text-[10px] text-text-tertiary'
                : 'text-[10px] text-status-warn'
            }
            title={
              resolve.ok
                ? `${resolve.provider_short ?? '?'}: ${resolve.model_id ?? '?'}`
                : (resolve.error ?? '')
            }
          >
            {resolve.ok
              ? `${t('chatZone.resolve.ok', { defaultValue: '默认' })}: ${resolve.provider_short}:${resolve.model_id} (${resolve.api_kind ?? 'openai-compat'})`
              : (resolve.error ?? t('chatZone.resolve.unconfigured', { defaultValue: '未配置' }))}
          </span>
        )}
      </div>

      {/* Input — single textarea, no surrounding controls blocking
           focus. */}
      <form onSubmit={onSubmit} className="flex shrink-0 flex-col gap-2 px-4 py-2">
        <textarea
          value={task}
          onChange={(e) => setTask(e.target.value)}
          onKeyDown={onKeyDown}
          disabled={sending}
          rows={4}
          placeholder={t('chatZone.taskPlaceholder', { defaultValue: '跟角色说点什么…（Ctrl+Enter 发送）' })}
          className="w-full resize-y rounded border border-border bg-surface-2 px-3 py-2 font-mono text-sm placeholder:text-text-secondary focus:border-chief focus:outline-none focus:ring-2 focus:ring-chief/50 disabled:opacity-50"
        />
        <div className="flex items-center justify-between gap-2">
          <span className="text-xs text-text-secondary">
            {sending
              ? t('chatZone.running', { defaultValue: '运行中…' })
              : done
                ? `${t('chatZone.done', { defaultValue: '已结束：' })}${status ?? '?'}`
                : t('chatZone.ready', { defaultValue: '准备就绪' })}
          </span>
          <button
            type="submit"
            disabled={sending || task.trim().length === 0}
            className="rounded bg-chief px-4 py-1.5 text-sm font-medium text-white transition-colors hover:bg-chief/90 disabled:pointer-events-none disabled:opacity-50"
          >
            {sending
              ? t('chatZone.sending', { defaultValue: '发送中…' })
              : t('chatZone.send', { defaultValue: '发送' })}
          </button>
        </div>
        {error && (
          <p className="text-xs text-red-400" role="alert">
            ⚠ {error}
          </p>
        )}
      </form>

      {/* Body: transcript + tool timeline */}
      <div className="grid min-h-0 flex-1 grid-cols-1 gap-2 px-4 pb-3 lg:grid-cols-[2fr_1fr]">
        {/* Streaming transcript */}
        <div
          ref={transcriptRef}
          className="flex min-h-0 flex-col gap-2 overflow-y-auto rounded border border-border bg-surface-2 p-3"
          aria-live="polite"
        >
          {text.length === 0 && !sending && (
            <p className="text-xs text-text-secondary">{t('chatZone.waiting', { defaultValue: '等待输入…（输出会在这里流式显示）' })}</p>
          )}
          {text && (
            <pre className="whitespace-pre-wrap break-words font-sans text-sm leading-relaxed text-text-primary">
              {text}
            </pre>
          )}
          {sending && text.length === 0 && (
            <p className="text-xs italic text-text-secondary">{t('chatZone.waitingModel', { defaultValue: '… 正在等待模型响应' })}</p>
          )}
        </div>

        {/* Tool timeline */}
        <div className="flex min-h-0 flex-col gap-1 overflow-y-auto rounded border border-border bg-surface-2 p-3" aria-label="工具">
          <div className="mb-1 text-[10px] uppercase tracking-wide text-text-secondary">
            {t('chatZone.tools', { defaultValue: '工具' })} ({toolEvents.length === 0
              ? t('chatZone.toolsEmpty', { defaultValue: '（暂无工具调用）' })
              : ''}
          </div>
          {toolEvents.length === 0 ? null : (
            <ol className="space-y-1 text-xs">
              {toolEvents.map((ev, i) => (
                <li key={i} className="rounded border border-border bg-surface-1 px-2 py-1 font-mono">
                  {ev.kind === 'tool_started' && (
                    <span>
                      ▶ {String(ev.call.name)} {String((ev.call.args as Record<string, unknown>)?.['command'] ?? '').slice(0, 80)}
                    </span>
                  )}
                  {ev.kind === 'tool_finished' && (
                    <span>
                      ✓ {String(ev.preview)} ({String(ev.elapsed_ms)}ms)
                    </span>
                  )}
                </li>
              ))}
            </ol>
          )}
        </div>
      </div>

      {/* Error log toggled out for brevity; if event stream yields an
          error-level tool call, the controller is via the error
          banner above. */}
      <details className="border-t border-border bg-surface-2/60 px-4 py-2 text-xs">
        <summary className="cursor-pointer text-text-secondary">{t('chatZone.logs', { defaultValue: '日志' })}</summary>
        <pre className="mt-2 max-h-32 overflow-y-auto whitespace-pre-wrap break-words font-mono text-text-primary">
          {events.length === 0 ? t('chatZone.noLogs', { defaultValue: '没有日志。' }) : null}
        </pre>
      </details>
    </section>
  );
}