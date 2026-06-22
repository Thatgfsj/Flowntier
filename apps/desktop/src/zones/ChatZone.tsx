/**
 * ChatZone — v0.3 progressive chat panel.
 *
 * Sits next to the existing CommandDock as a second input
 * surface. The user types a free-form task; we send it to the
 * agent-core via the pipe-server `/api/run_task` endpoint, and
 * stream the resulting `AgentEvent`s back through `useAgentStream`.
 *
 * Progressive: reuses the existing zone chrome, no IDE rewrite.
 * Visible affordances:
 *   - role picker (首席 / 工匠 / 缺陷猎手 / 质检师 / 军师 / 传令官)
 *   - provider picker (OpenAI / Anthropic-compatible / 自定义)
 *   - multiline input
 *   - send button (also Ctrl/Cmd+Enter)
 *   - streaming assistant transcript
 *   - tool timeline (with command preview)
 *   - token usage + final status
 */
import { useCallback, useMemo, useRef, useState, type FormEvent, type KeyboardEvent } from 'react';
import { useAgentStream, type AgentEvent } from '../hooks/useAgentStream.js';

interface RoleSpec {
  id: string;
  label: string;
  hint: string;
}

const ROLES: RoleSpec[] = [
  { id: 'agent:chief',    label: '首席',   hint: '拆任务、调度、汇总' },
  { id: 'agent:worker',   label: '工匠',   hint: '写代码、改文件、跑命令' },
  { id: 'agent:critic:a', label: '缺陷猎手', hint: '挖 bug、安全、边界' },
  { id: 'agent:critic:b', label: '质检师', hint: '命名、抽象、文档' },
  { id: 'agent:planner',  label: '军师',   hint: '方案、接口、验收' },
  { id: 'agent:reporter', label: '传令官', hint: '给用户写最终总结' },
];

interface ProviderSpec {
  id: string;
  label: string;
  base_url: string;
}

const PROVIDERS: ProviderSpec[] = [
  { id: 'openai_compat', label: 'OpenAI 兼容', base_url: 'https://api.openai.com/v1' },
  { id: 'openai_compat', label: 'DeepSeek',    base_url: 'https://api.deepseek.com/v1' },
  { id: 'openai_compat', label: 'Moonshot',    base_url: 'https://api.moonshot.cn/v1' },
  { id: 'openai_compat', label: '自定义 relay', base_url: '' },
];

const DEFAULT_PROVIDER: ProviderSpec =
  PROVIDERS[0] ?? { id: 'openai_compat', label: 'OpenAI 兼容', base_url: 'https://api.openai.com/v1' };

export interface ChatZoneProps {
  /** Optional default provider key (env var name) to use for the API key. */
  defaultKeyEnvVar?: string;
  /** Optional default model id. */
  defaultModel?: string;
}

export function ChatZone({
  defaultKeyEnvVar = 'OPENAI_API_KEY',
  defaultModel = 'gpt-4o-mini',
}: ChatZoneProps) {
  const [task, setTask] = useState('');
  const [role, setRole] = useState<string>('agent:chief');
  const [provider, setProvider] = useState<ProviderSpec>(DEFAULT_PROVIDER);
  const [baseUrl, setBaseUrl] = useState(DEFAULT_PROVIDER.base_url);
  const [model, setModel] = useState(defaultModel);
  const [keyEnvVar, setKeyEnvVar] = useState(defaultKeyEnvVar);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const { events, text, done, status, reset } = useAgentStream();
  const transcriptRef = useRef<HTMLDivElement>(null);

  const toolEvents = useMemo(
    () => events.filter((e) => e.kind === 'tool_started' || e.kind === 'tool_finished'),
    [events],
  );

  const send = useCallback(async () => {
    const trimmed = task.trim();
    if (!trimmed || sending) return;
    setError(null);
    reset();
    setSending(true);
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      // The Tauri shell doesn't yet expose /api/run_task directly
      // as a command; once the v0.3 backend wires this in, we
      // call `run_agent_task` here. Until then, fall through to
      // a JSON-RPC call via the pipe if the backend adds a
      // `pipe_rpc` command.
      const ok = await invoke<{ ok: boolean; error?: string }>('run_agent_task', {
        body: {
          task: trimmed,
          role,
          provider_kind: provider.id,
          base_url: baseUrl,
          model,
          api_key_env: keyEnvVar,
        },
      });
      if (!ok?.ok) {
        setError(ok?.error ?? '运行时未确认成功');
      }
    } catch (e) {
      setError(typeof e === 'string' ? e : (e as Error).message);
    } finally {
      setSending(false);
    }
  }, [task, sending, reset, role, provider.id, baseUrl, model, keyEnvVar]);

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
          <span className="text-sm text-text-secondary">v0.3 渐进式 · 直接驱动 agent-core</span>
        </div>
        <button
          type="button"
          onClick={reset}
          className="rounded border border-border px-2 py-0.5 text-xs text-text-secondary hover:bg-surface-1"
        >
          清空
        </button>
      </header>

      {/* Controls */}
      <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-border bg-surface-2/60 px-4 py-2 text-xs">
        <label className="flex items-center gap-1">
          <span className="text-text-secondary">角色</span>
          <select
            value={role}
            onChange={(e) => setRole(e.target.value)}
            disabled={sending}
            className="rounded border border-border bg-surface-1 px-2 py-1 disabled:opacity-50"
          >
            {ROLES.map((r) => (
              <option key={r.id} value={r.id} title={r.hint}>
                {r.label}
              </option>
            ))}
          </select>
        </label>
        <label className="flex items-center gap-1">
          <span className="text-text-secondary">Provider</span>
          <select
            value={provider.label}
            onChange={(e) => {
              const next = PROVIDERS.find((p) => p.label === e.target.value);
              if (next !== undefined) {
                setProvider(next);
                setBaseUrl(next.base_url);
              }
            }}
            disabled={sending}
            className="rounded border border-border bg-surface-1 px-2 py-1 disabled:opacity-50"
          >
            {PROVIDERS.map((p, i) => (
              <option key={`${p.label}-${i}`} value={p.label}>
                {p.label}
              </option>
            ))}
          </select>
        </label>
        <label className="flex items-center gap-1">
          <span className="text-text-secondary">Base URL</span>
          <input
            type="text"
            value={baseUrl}
            onChange={(e) => setBaseUrl(e.target.value)}
            disabled={sending}
            className="w-72 rounded border border-border bg-surface-1 px-2 py-1 font-mono text-xs disabled:opacity-50"
            placeholder="https://api.openai.com/v1"
          />
        </label>
        <label className="flex items-center gap-1">
          <span className="text-text-secondary">Model</span>
          <input
            type="text"
            value={model}
            onChange={(e) => setModel(e.target.value)}
            disabled={sending}
            className="w-32 rounded border border-border bg-surface-1 px-2 py-1 font-mono text-xs disabled:opacity-50"
          />
        </label>
        <label className="flex items-center gap-1">
          <span className="text-text-secondary">API Key env</span>
          <input
            type="text"
            value={keyEnvVar}
            onChange={(e) => setKeyEnvVar(e.target.value)}
            disabled={sending}
            className="w-40 rounded border border-border bg-surface-1 px-2 py-1 font-mono text-xs disabled:opacity-50"
            placeholder="OPENAI_API_KEY"
          />
        </label>
      </div>

      {/* Input */}
      <form onSubmit={onSubmit} className="flex shrink-0 flex-col gap-2 px-4 py-2">
        <textarea
          value={task}
          onChange={(e) => setTask(e.target.value)}
          onKeyDown={onKeyDown}
          disabled={sending}
          rows={3}
          placeholder="跟角色说点什么…（Ctrl+Enter 发送）  例如：给 main.rs 加一个 --verbose flag，把 println 改成 eprintln"
          className="w-full resize-y rounded border border-border bg-surface-2 px-3 py-2 font-mono text-sm placeholder:text-text-secondary focus:border-chief focus:outline-none disabled:opacity-50"
        />
        <div className="flex items-center justify-between gap-2">
          <span className="text-xs text-text-secondary">
            {sending ? '运行中…' : done ? `已结束：${status ?? '?'}` : '准备就绪'}
          </span>
          <button
            type="submit"
            disabled={sending || task.trim().length === 0}
            className="rounded bg-chief px-4 py-1.5 text-sm font-medium text-white transition-colors hover:bg-chief/90 disabled:pointer-events-none disabled:opacity-50"
          >
            {sending ? '发送中…' : '发送'}
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
            <p className="text-xs text-text-secondary">
              等待输入…（输出会在这里流式显示）
            </p>
          )}
          {text && (
            <pre className="whitespace-pre-wrap break-words font-sans text-sm leading-relaxed text-text-primary">
              {text}
            </pre>
          )}
          {sending && text.length === 0 && (
            <p className="text-xs italic text-text-secondary">… 正在等待模型响应</p>
          )}
        </div>

        {/* Tool timeline */}
        <aside
          className="flex min-h-0 flex-col gap-1 overflow-y-auto rounded border border-border bg-surface-2 p-3 text-xs"
          aria-label="工具调用时间线"
        >
          <h3 className="mb-1 font-mono text-[10px] uppercase tracking-wider text-text-secondary">
            工具
          </h3>
          {toolEvents.length === 0 && (
            <p className="text-text-secondary">（暂无工具调用）</p>
          )}
          {toolEvents.map((e, i) => (
            <ToolTimelineRow key={i} ev={e} />
          ))}
        </aside>
      </div>
    </section>
  );
}

function ToolTimelineRow({ ev }: { ev: AgentEvent }) {
  if (ev.kind === 'tool_started') {
    return (
      <div className="flex items-start gap-1.5 font-mono">
        <span className="text-text-secondary">▸</span>
        <span className="shrink-0 text-chief">{ev.call.name}</span>
        <span className="truncate text-text-secondary">
          {summarizeArgs(ev.call.args)}
        </span>
      </div>
    );
  }
  if (ev.kind === 'tool_finished') {
    return (
      <div className="flex items-start gap-1.5 font-mono">
        <span className={ev.is_error ? 'text-red-400' : 'text-green-400'}>
          {ev.is_error ? '✗' : '✓'}
        </span>
        <span className="shrink-0 text-text-secondary">{ev.elapsed_ms}ms</span>
        <span className="truncate text-text-primary" title={ev.preview}>
          {ev.preview}
        </span>
      </div>
    );
  }
  return null;
}

function summarizeArgs(args: unknown): string {
  if (args === null || args === undefined) return '';
  if (typeof args === 'string') return args;
  try {
    const s = JSON.stringify(args);
    return s.length > 80 ? s.slice(0, 77) + '…' : s;
  } catch {
    return '';
  }
}
