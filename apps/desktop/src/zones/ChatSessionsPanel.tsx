/**
 * ChatSessionsPanel — v0.4.22 (event 000118, fix 6 hardening,
 * persistence).
 *
 * AIde-style collapsible list of past chat sessions. Sits at the
 * top of ChatZone, between the header and the mode toggle.
 * Click a session → loads its messages into ChatZone's
 * `chatHistory` state and marks it active. Click "+ 新对话" →
 * creates a fresh empty session and marks it active.
 *
 * Persistence is handled by the parent (ChatZone) via
 * `kvGet`/`kvSet`. The panel itself is "dumb" — it takes the
 * list and a few callbacks, renders, no I/O.
 */
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { ChatSession } from './chatSessions.js';

export interface ChatSessionsPanelProps {
  sessions: ChatSession[];
  activeId: string | null;
  onSelect: (id: string) => void;
  onCreate: () => void;
  onDelete: (id: string) => void;
  /**
   * v0.4.22 (event 000118, fix 6 persistence): default OPEN
   * for the active session. Collapsed by default if there are
   * no sessions yet. The chairman asked for "AIde-style
   * collapsible" — we ship a real disclosure, not a permanent
   * list.
   */
  defaultOpen?: boolean;
  loading?: boolean;
}

export function ChatSessionsPanel({
  sessions,
  activeId,
  onSelect,
  onCreate,
  onDelete,
  defaultOpen,
  loading = false,
}: ChatSessionsPanelProps) {
  const { t } = useTranslation();
  // Default: open if user has ≥1 session, closed otherwise.
  const [open, setOpen] = useState<boolean>(defaultOpen ?? sessions.length > 0);

  // Show top 8 most recent in the panel; full list still lives
  // in storage. Older sessions are still selectable via the
  // "+ 全部" link in a future patch; for v0.4.22 the top-8 cap
  // is enough — the chairman said "AIde" as a reference, not
  // a pixel-perfect replica.
  const visible = sessions.slice(0, 8);

  return (
    <div className="shrink-0 border-b border-border bg-surface-1 px-4 py-2 text-xs">
      <div className="flex items-center justify-between gap-2">
        <button
          type="button"
          onClick={() => setOpen((v) => !v)}
          className="flex items-center gap-1 text-text-secondary hover:text-text-primary"
          aria-expanded={open}
          aria-controls="chat-sessions-list"
        >
          <span aria-hidden="true">{open ? '▾' : '▸'}</span>
          <span className="font-mono text-[10px] uppercase tracking-wide">
            {t('chatZone.sessionsTitle', { defaultValue: '对话历史' })}
          </span>
          <span className="text-text-secondary">
            ({sessions.length})
          </span>
        </button>
        <button
          type="button"
          onClick={onCreate}
          className="rounded border border-border px-2 py-0.5 text-[11px] text-text-secondary hover:bg-surface-2"
          aria-label={t('chatZone.sessionsNew', { defaultValue: '新建对话' })}
        >
          + {t('chatZone.sessionsNew', { defaultValue: '新对话' })}
        </button>
      </div>
      {open && (
        <ul
          id="chat-sessions-list"
          className="mt-2 flex max-h-40 flex-col gap-1 overflow-y-auto"
        >
          {loading && (
            <li className="text-text-secondary">…</li>
          )}
          {!loading && visible.length === 0 && (
            <li className="text-text-secondary">
              {t('chatZone.sessionsEmpty', { defaultValue: '还没有对话 — 发一条消息即可创建' })}
            </li>
          )}
          {visible.map((s) => {
            const isActive = s.id === activeId;
            return (
              <li
                key={s.id}
                className={
                  'group flex items-center gap-2 rounded px-2 py-1 ' +
                  (isActive
                    ? 'bg-accent/10 text-text-primary'
                    : 'text-text-secondary hover:bg-surface-2 hover:text-text-primary')
                }
              >
                <button
                  type="button"
                  onClick={() => onSelect(s.id)}
                  className="flex-1 truncate text-left"
                  aria-current={isActive ? 'true' : undefined}
                >
                  <span className="block truncate text-[12px]">{s.title}</span>
                  <span className="block text-[10px] text-text-secondary">
                    {new Date(s.updatedAt).toLocaleString()}
                    {' · '}
                    {s.mode === 'chat'
                      ? t('chatZone.modeChat', { defaultValue: '对话' })
                      : t('chatZone.modeWorkflow', { defaultValue: '工作流' })}
                    {' · '}
                    {s.messages.length} {t('chatZone.sessionsTurns', { defaultValue: '轮' })}
                  </span>
                </button>
                <button
                  type="button"
                  onClick={(e) => {
                    e.stopPropagation();
                    if (confirm(t('chatZone.sessionsConfirmDelete', {
                      defaultValue: '删除这个对话?',
                    }))) {
                      onDelete(s.id);
                    }
                  }}
                  className="hidden rounded px-1 text-[10px] text-text-secondary hover:bg-status-failed/20 hover:text-status-failed group-hover:inline-block"
                  aria-label={t('chatZone.sessionsDelete', { defaultValue: '删除' })}
                >
                  ✕
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
