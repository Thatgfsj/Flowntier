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
import {
  getRoleResolveStatus,
  kvGet,
  kvSet,
  runAgentTask,
  type ChatTurnMessage,
  type RoleResolveStatus,
} from '../lib/api.js';
import { fallbackSummary as fallbackSummaryFromEvents } from './chatFallback.js';
import {
  findSession,
  loadActiveId,
  loadSessions,
  newSession as buildNewSession,
  removeSession,
  sortByUpdatedAt,
  STORAGE_KEY_ACTIVE,
  STORAGE_KEY_SESSIONS,
  touchSession,
  upsertSession,
  type ChatSession,
} from './chatSessions.js';
import { ChatSessionsPanel } from './ChatSessionsPanel.js';

interface RoleSpec {
  id: string;
  /** i18n key suffix (e.g. "chief", "worker", "criticA") — the
   *  component resolves label + hint via t() at render time. */
  i18nKey: string;
}

// v0.4.22 (event 000118, fix 2): the bottom of ChatZone used
// to have a giant empty gap below the collapsible logs —
// the chairman asked us to fill it with two compact blocks:
//   left  = 当前就绪 <当前角色名> (worker 不显示)
//   right = 工具 (tool-call timeline)
//
// To render the left block with a real role name + status, we
// accept a `activeAgent` prop describing which agent card is
// currently driving the workflow (the chairman's directive is
// that the user should see "what role is working right now"
// without scrolling back to the dashboard). The App passes
// null when no workflow is running OR when the active role is
// the worker (which the chairman explicitly excluded).
interface ActiveAgentInfo {
  /** Short role id like 'chief' / 'critic-a' / 'critic-b' — the
   *  App maps the PHASE → agent role, then ChatZone only
   *  shows the block when this is one of the head roles. */
  role: 'chief' | 'critic-a' | 'critic-b';
  /** i18n label for the role (already translated by App). */
  label: string;
  /** status string used by the dashboard (idle / thinking / speaking). */
  status: 'idle' | 'thinking' | 'speaking';
  /** optional phase label e.g. "5-开发" to make the block self-contained. */
  phaseLabel?: string;
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
  /**
   * event 000110 (fix A): ChatZone now owns its own collapse button
   * instead of an absolutely-positioned overlay floating over the
   * header (which made "清空" and "▾ 折叠" overlap visually in the
   * top-right corner). Pass `setChatOpen(false)` so the header
   * button can collapse the panel itself.
   */
  onCollapse?: () => void;
  /**
   * v0.4.22 (event 000118, fix 2): App.tsx passes the role the
   * workflow is currently driving (chief / critic-a / critic-b).
   * ChatZone renders "当前就绪: <role> · <status> · <phase>"
   * in the gap below the input + logs. Null = no workflow, or
   * worker — in either case the block is hidden so worker-only
   * workflows don't get a noisy "ready" line.
   */
  activeAgent?: ActiveAgentInfo | null;
}

export function ChatZone({ onCollapse, activeAgent = null }: ChatZoneProps = {}) {
  const { t } = useTranslation();
  const [task, setTask] = useState('');
  const [role, setRole] = useState<string>('agent:chief');
  // v0.4.22 (event 000118, fix 6): two send modes.
  //   - 'workflow': default; goes through the 8-phase orchestrator
  //     (run_workflow). For new tasks the user wants the full
  //     plan+critic+worker dispatch flow.
  //   - 'chat': bypasses the orchestrator and runs a single
  //     agent (run_agent_task). For quick back-and-forth like
  //     "打开给我看看" — without this, even a single sentence
  //     falls into Phase 1's clarification loop because the
  //     chief gets a fresh LLM context with no memory.
  const [mode, setMode] = useState<'workflow' | 'chat'>('workflow');
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [resolve, setResolve] = useState<RoleResolveStatus | null>(null);
  // v0.4.22 (event 000118, fix 6): rolling transcript of the
  // current 对话 session. We append user + assistant turns on
  // every send and pass `slice(-N)` (last N turns) to the
  // backend so the agent has memory of the conversation. Reset
  // when the user switches modes or hits 清空.
  const [chatHistory, setChatHistory] = useState<ChatTurnMessage[]>([]);
  // Bound the in-flight history so a long conversation can't
  // blow past the model's context window. 12 turns × ~2K chars
  // is roughly 24K tokens — well under the cheapest model.
  const MAX_CHAT_HISTORY = 12;

  // v0.4.22 (event 000118, fix 6 persistence): durable chat
  // sessions. We persist the full list under STORAGE_KEY_SESSIONS
  // and the active session id under STORAGE_KEY_ACTIVE. The
  // `chatHistory` above is the working copy of the active
  // session's messages — every send + every assistant reply
  // updates both the in-memory session and the persisted blob.
  const [sessions, setSessions] = useState<ChatSession[]>([]);
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [sessionsLoading, setSessionsLoading] = useState(true);

  const { events, text, done, status, reset } = useAgentStream();
  const transcriptRef = useRef<HTMLDivElement>(null);

  const toolEvents = useMemo(
    () => events.filter((e) => e.kind === 'tool_started' || e.kind === 'tool_finished'),
    [events],
  );

  // v0.4.22 (event 000118, fix 5): when phase 8 chief runs in
  // ~5 seconds and emits one batch of TextDeltas that all
  // land within a single React render, the user perceives
  // "ChatZone 没动静" — the transcript appears empty until
  // refresh. If we received a `done` event whose summary is
  // non-empty AND we never accumulated any text_delta, fall
  // back to displaying the summary so the chairman still
  // sees the phase-8 report. Streaming text always wins;
  // this is only the empty-text fallback. The pure helper
  // lives in `chatFallback.ts` so we can unit-test it.
  const fallbackText = useMemo(
    () => fallbackSummaryFromEvents(events, text),
    [events, text],
  );

  // v0.4.22 (event 000118, fix 6 persistence): load sessions
  // on mount. Runs exactly once; the loaders in `chatSessions`
  // are tolerant of corrupt storage (see vitest) so a
  // half-written SQLite blob can't brick the UI.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      const [rawSessions, rawActive] = await Promise.all([
        kvGet<unknown>(STORAGE_KEY_SESSIONS),
        kvGet<unknown>(STORAGE_KEY_ACTIVE),
      ]);
      if (cancelled) return;
      const loaded = loadSessions(rawSessions);
      const activeId = loadActiveId(rawActive);
      // Cross-check: if the active id no longer exists in the
      // list (corruption / manually deleted via dev tools),
      // drop the active pointer.
      const validActive = activeId && findSession(loaded, activeId)
        ? activeId
        : null;
      setSessions(sortByUpdatedAt(loaded));
      setActiveSessionId(validActive);
      // Pre-load the chatHistory if we found an active session.
      if (validActive) {
        const s = findSession(loaded, validActive);
        if (s) setChatHistory(s.messages);
      }
      setSessionsLoading(false);
    })();
    return () => { cancelled = true; };
  }, []);

  // v0.4.22 (event 000118, fix 6 persistence): when the active
  // session's messages change, mirror them into the session
  // object + bump updatedAt + persist. We track chatHistory
  // changes via a ref so this effect doesn't loop on itself.
  useEffect(() => {
    if (sessionsLoading) return;
    if (!activeSessionId) return;
    // Only update if the active session exists and its
    // messages actually differ.
    const current = findSession(sessions, activeSessionId);
    if (!current) return;
    if (current.messages === chatHistory) return;
    const updated: ChatSession = touchSession({
      ...current,
      messages: chatHistory,
    });
    setSessions((prev) => sortByUpdatedAt(upsertSession(prev, updated)));
    // chatHistory is intentionally NOT a dep: we only want to
    // react to its changes, not re-run on the same ref.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [chatHistory, activeSessionId, sessionsLoading]);

  // v0.4.22 (event 000118, fix 6 persistence): persist on
  // every change. We don't await — kvSet is fire-and-forget
  // for the user; failures are logged but never block the UI.
  useEffect(() => {
    if (sessionsLoading) return;
    void kvSet(STORAGE_KEY_SESSIONS, sessions);
  }, [sessions, sessionsLoading]);
  useEffect(() => {
    if (sessionsLoading) return;
    void kvSet(STORAGE_KEY_ACTIVE, activeSessionId);
  }, [activeSessionId, sessionsLoading]);

  // v0.4.20 (event 000056): the scheduler emits a special
  // AgentEvent::Done { status: "QUOTA_NUDGE:<role>:<model>" } when
  // a chief (role, model) pair is marked rate_limited. Surface the
  // nudge inline as a yellow banner above the chat input.
  const quotaNudge = useMemo(() => {
    const nudge = events.find(
      (e) => e.kind === 'done' && typeof e.status === 'string'
        && e.status.startsWith('QUOTA_NUDGE:'),
    );
    if (!nudge || nudge.kind !== 'done') return null;
    return {
      summary: nudge.summary ?? '',
      status: nudge.status,
    };
  }, [events]);

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

  // v0.4.22 (event 000118, fix 6): when the stream closes
  // with `done`, append an assistant turn to chatHistory so
  // the next 对话-mode send has it in memory. We use the
  // streamed `text` if it's non-empty, else fall back to the
  // `summary` (event 000118 fix 5 path). Skipped in workflow
  // mode because the orchestrator builds its own context.
  useEffect(() => {
    if (mode !== 'chat') return;
    if (!done) return;
    const assistantTurn = text.trim() || (fallbackText ?? '').trim();
    if (!assistantTurn) return;
    setChatHistory((h) => {
      // Avoid duplicate consecutive assistant turns if the
      // effect re-fires for the same `done` (e.g. StrictMode).
      const last = h[h.length - 1];
      if (last && last.role === 'assistant' && last.content === assistantTurn) return h;
      const next = [...h, { role: 'assistant' as const, content: assistantTurn }];
      return next.slice(-MAX_CHAT_HISTORY);
    });
  }, [done, mode, text, fallbackText]);

  const send = useCallback(async () => {
    const trimmed = task.trim();
    if (!trimmed || sending) return;
    setError(null);
    reset();
    setSending(true);

    // v0.4.22 (event 000118, fix 6 persistence): if there's no
    // active session, this send creates one. We do it BEFORE
    // runAgentTask so the panel shows the new entry while the
    // assistant is still streaming. The first user turn becomes
    // the session's title.
    let sessionId = activeSessionId;
    if (!sessionId) {
      const created = buildNewSession(trimmed, mode);
      setSessions((prev) => sortByUpdatedAt(upsertSession(prev, created)));
      setActiveSessionId(created.id);
      sessionId = created.id;
    }

    // v0.4.22 (event 000118, fix 6): snapshot chatHistory BEFORE
    // we append the new user turn so the backend receives the
    // conversation as it stood a moment ago (avoids an
    // off-by-one where the user's current message appears in
    // both the chat_history slice and the task arg).
    const historyToSend = chatHistory.slice(-MAX_CHAT_HISTORY);

    try {
      if (mode === 'chat') {
        // Single-agent chat: bypass orchestrator, send memory.
        // The chairman's fix6 example: "打开给我看看" goes
        // straight to /api/run_task with the prior turns; the
        // chief reads README and answers.
        await runAgentTask({
          task: trimmed,
          role,
          chat_history: historyToSend,
        });
      } else {
        // 8-phase workflow (default). chat_history is irrelevant
        // here — phase 1 builds its own context from user_request.
        const ok = await invoke<{ ok: boolean; error?: string; role?: string; hint?: string; status?: string; wf_id?: string; summary?: string }>(
          'run_workflow',
          { body: { task: trimmed } },
        );
        if (!ok?.ok) {
          const tail = ok?.error
            ? `${ok.error}${ok.hint ? ` — ${ok.hint}` : ''}`
            : ok?.status ?? '运行时未确认成功';
          setError(tail);
        }
      }
      // v0.4.22 (event 000118, fix 6): append the user turn so
      // the next 对话-mode send has it in memory. We append
      // the assistant turn in a separate effect keyed on `done`
      // so we capture the streamed summary even when the
      // backend didn't stream text.
      setChatHistory((h) => {
        const next = [...h, { role: 'user' as const, content: trimmed }];
        return next.slice(-MAX_CHAT_HISTORY);
      });
    } catch (e) {
      // Async pipe failure (server not reachable, panic, etc.).
      const msg = typeof e === 'string' ? e : (e as Error).message;
      // 'HTTP 500: {...}' style — strip the JSON noise.
      const trimmed2 = msg?.replace(/^HTTP \d+:\s*/, '').slice(0, 240);
      setError(trimmed2 ?? 'unknown error');
    } finally {
      setSending(false);
    }
  }, [task, sending, reset, role, mode, chatHistory, activeSessionId]);

  // v0.4.22 (event 000118, fix 6 persistence): session action
  // handlers. Select / create / delete. Each is one effect-free
  // setState cascade so the persist effects fire correctly.

  const selectSession = useCallback((id: string) => {
    const s = findSession(sessions, id);
    if (!s) return;
    setActiveSessionId(id);
    setChatHistory(s.messages);
    setMode(s.mode);
    reset();
    setError(null);
  }, [sessions]);

  const createNewSession = useCallback(() => {
    setActiveSessionId(null);
    setChatHistory([]);
    setMode(mode);  // keep current mode; user can change after
    reset();
    setError(null);
  }, [mode]);

  const deleteSession = useCallback((id: string) => {
    setSessions((prev) => removeSession(prev, id));
    if (activeSessionId === id) {
      // Active session was deleted — clear the chat so the
      // next send starts a brand new session.
      setActiveSessionId(null);
      setChatHistory([]);
    }
  }, [activeSessionId]);

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
      {/* Header — event 000110 (fix A): collapse button now lives
          inline (left of clear), so the top-right corner only
          carries ONE action button ("清空") instead of two
          ("清空" + floating "▾ 折叠") that visually overlapped. */}
      <header className="flex shrink-0 items-center justify-between gap-3 border-b border-border bg-surface-2 px-4 py-2">
        <div className="flex items-center gap-2">
          <span className="font-mono text-xs text-text-secondary">ChatZone ▸</span>
          <span className="text-sm text-text-secondary">{t('chatZone.subtitle', { defaultValue: '由设置中的角色分配配置驱动' })}</span>
        </div>
        <div className="flex items-center gap-1">
          {onCollapse && (
            <button
              type="button"
              onClick={onCollapse}
              className="rounded border border-border px-2 py-0.5 text-xs text-text-secondary hover:bg-surface-1"
              aria-label={t('app.aria.chatCollapse', { defaultValue: '折叠 ChatZone' })}
            >
              {t('app.chatCollapse', { defaultValue: '▾ 折叠' })}
            </button>
          )}
          <button
            type="button"
            onClick={reset}
            className="rounded border border-border px-2 py-0.5 text-xs text-text-secondary hover:bg-surface-1"
          >
            {t('chatZone.clear', { defaultValue: '清空' })}
          </button>
        </div>
      </header>

      {/* v0.4.22 (event 000118, fix 6 persistence): AIde-style
          collapsible sessions list at the top of ChatZone. */}
      <ChatSessionsPanel
        sessions={sessions}
        activeId={activeSessionId}
        onSelect={selectSession}
        onCreate={createNewSession}
        onDelete={deleteSession}
        loading={sessionsLoading}
      />

      {/* Controls — only the role picker remains; everything else
          moved to Settings. */}
      <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-border bg-surface-2/60 px-4 py-2 text-xs">
        {/* v0.4.22 (event 000118, fix 6): mode toggle. Default
            is 'workflow' (preserves the existing 8-phase flow).
            Switching to 'chat' sends straight to /api/run_task
            with chat_history so quick back-and-forth doesn't
            fall into Phase 1's clarification loop. */}
        <div className="inline-flex overflow-hidden rounded border border-border text-[11px]" role="group" aria-label={t('chatZone.modeLabel', { defaultValue: '发送模式' })}>
          <button
            type="button"
            onClick={() => setMode('workflow')}
            disabled={sending}
            className={
              mode === 'workflow'
                ? 'bg-chief px-2 py-1 font-semibold text-white disabled:opacity-50'
                : 'bg-surface-1 px-2 py-1 text-text-secondary hover:bg-surface-3 disabled:opacity-50'
            }
            aria-pressed={mode === 'workflow'}
            title={t('chatZone.modeWorkflowHint', { defaultValue: '8 阶段编排, 主理+策划+找茬+实施+交付, 适合正式任务' })}
          >
            {t('chatZone.modeWorkflow', { defaultValue: '启动工作流' })}
          </button>
          <button
            type="button"
            onClick={() => setMode('chat')}
            disabled={sending}
            className={
              mode === 'chat'
                ? 'bg-chief px-2 py-1 font-semibold text-white disabled:opacity-50'
                : 'bg-surface-1 px-2 py-1 text-text-secondary hover:bg-surface-3 disabled:opacity-50'
            }
            aria-pressed={mode === 'chat'}
            title={t('chatZone.modeChatHint', { defaultValue: '单角色对话, 主理有记忆, 适合追问/查资料' })}
          >
            {t('chatZone.modeChat', { defaultValue: '对话' })}
          </button>
        </div>
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

      {/* v0.4.20 (event 000056): Quota nudge banner. Visible when
          the scheduler flipped a (chief, model) pair to
          rate_limited and emitted the chairman-mandated text. */}
      {quotaNudge && (
        <div
          role="status"
          className="mx-4 mb-1 rounded-md border border-status-warn/40 bg-status-warn/10 px-3 py-2 text-xs text-status-warn"
        >
          <div className="font-semibold">
            {t('chatZone.quotaNudgeTitle', { defaultValue: 'Quota Refresh' })}
          </div>
          <div className="text-text-secondary">
            {quotaNudge.summary
              || t('chatZone.quotaNudge', {
                defaultValue: 'AI 之前疑似到达上限，目前已经刷新，检查工作进度并且继续工作',
              })}
          </div>
        </div>
      )}

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
{text.length === 0 && !sending && !fallbackText && (
            <p className="text-xs text-text-secondary">{t('chatZone.waiting', { defaultValue: '等待输入…（输出会在这里流式显示）'})}</p>
          )}
          {text && (
            <pre className="whitespace-pre-wrap break-words font-sans text-sm leading-relaxed text-text-primary">
              {text}
            </pre>
          )}
          {!text && fallbackText && (
            // v0.4.22 (event 000118, fix 5): empty-stream
            // fallback. Phase 8 chief can complete in ~5s
            // with the text arriving faster than the React
            // render cycle; without this the transcript
            // looks blank even though the workflow finished.
            // The badge tells the chairman this is the
            // post-hoc snapshot, not live streaming.
            <>
              <p className="text-[10px] uppercase tracking-wide text-text-tertiary">
                {t('chatZone.summaryFallback', { defaultValue: '汇报快照（未流式）' })}
              </p>
              <pre className="whitespace-pre-wrap break-words font-sans text-sm leading-relaxed text-text-primary">
                {fallbackText}
              </pre>
            </>
          )}
          {sending && text.length === 0 && !fallbackText && (
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

      {/* v0.4.22 (event 000118, fix 2): fill the empty space
          below the collapsible logs with two compact blocks:

            left  = 当前就绪 + 当前工作角色（仅 chief / critic-a /
                    critic-b；worker 按主席指示不展示）
            right = 工具 (tool-call timeline; reuses toolEvents so
                    the live transcript and the bottom panel agree)

          Each block is a fixed-height scroll region so the
          ChatZone panel height stays bounded and the empty
          space above this row is closed. */}
      <div
        className="grid shrink-0 grid-cols-1 gap-2 border-t border-border bg-surface-1 px-4 py-2 lg:grid-cols-2"
        aria-label="当前就绪与工具"
      >
        {activeAgent ? (
          <div className="flex min-h-[88px] flex-col gap-1 overflow-y-auto rounded border border-border bg-surface-2 p-2">
            <div className="flex items-center justify-between gap-2">
              <span className="text-[10px] uppercase tracking-wide text-text-secondary">
                {t('chatZone.readyTitle', { defaultValue: '当前就绪' })}
              </span>
              {activeAgent.phaseLabel && (
                <span className="font-mono text-[10px] text-text-secondary">
                  {activeAgent.phaseLabel}
                </span>
              )}
            </div>
            <div className="flex items-center gap-2">
              <span
                className={
                  activeAgent.status === 'thinking'
                    ? 'rounded bg-chief/20 px-1.5 py-0.5 font-mono text-[11px] font-semibold text-chief'
                    : activeAgent.status === 'speaking'
                      ? 'rounded bg-status-done/20 px-1.5 py-0.5 font-mono text-[11px] font-semibold text-status-done'
                      : 'rounded bg-surface-3 px-1.5 py-0.5 font-mono text-[11px] text-text-secondary'
                }
              >
                {activeAgent.label}
              </span>
              <span className="text-[11px] text-text-secondary">
                {t(`chatZone.agentStatus.${activeAgent.status}`, {
                  defaultValue:
                    activeAgent.status === 'thinking'
                      ? '思考中…'
                      : activeAgent.status === 'speaking'
                        ? '发言中…'
                        : '空闲',
                })}
              </span>
            </div>
            <p className="text-[11px] leading-snug text-text-tertiary">
              {t('chatZone.readyHint', {
                defaultValue: '当主理 / 找茬 / 审查在动时这里实时显示当前角色与阶段。',
              })}
            </p>
          </div>
        ) : (
          // No active head-role → worker-only or no workflow.
          // Chairman asked us to NOT show "current work" when the
          // worker is the one moving. Render a single-cell
          // placeholder so the grid still reserves the column.
          <div className="flex min-h-[88px] items-center justify-center rounded border border-dashed border-border bg-surface-2 p-2 text-[11px] text-text-secondary">
            {t('chatZone.readyEmpty', { defaultValue: '（当前阶段无主理角色在工作）' })}
          </div>
        )}
        <div className="flex min-h-[88px] flex-col gap-1 overflow-y-auto rounded border border-border bg-surface-2 p-2" aria-label="工具">
          <div className="mb-1 text-[10px] uppercase tracking-wide text-text-secondary">
            {t('chatZone.tools', { defaultValue: '工具' })}{' '}
            <span className="text-text-tertiary">
              ({toolEvents.length === 0
                ? t('chatZone.toolsEmpty', { defaultValue: '（暂无工具调用）' })
                : toolEvents.length}
              )
            </span>
          </div>
          {toolEvents.length === 0 ? null : (
            <ol className="space-y-1 text-[11px]">
              {toolEvents.slice(-8).reverse().map((ev, i) => (
                <li key={i} className="rounded border border-border bg-surface-1 px-2 py-1 font-mono">
                  {ev.kind === 'tool_started' && (
                    <span className="break-all">
                      ▶ {String(ev.call.name)} {String((ev.call.args as Record<string, unknown>)?.['command'] ?? '').slice(0, 60)}
                    </span>
                  )}
                  {ev.kind === 'tool_finished' && (
                    <span>
                      ✓ {String(ev.preview).slice(0, 60)} ({String(ev.elapsed_ms)}ms)
                    </span>
                  )}
                </li>
              ))}
            </ol>
          )}
        </div>
      </div>
    </section>
  );
}