/**
 * v0.4.22 (event 000118, fix 6 hardening, persistence):
 * Chat sessions = durable conversations that survive an app
 * restart. Modeled like "AIde" (the chairman's reference):
 *   - Each session has an id, a title (auto-derived from the
 *     first user turn), a list of messages, and timestamps.
 *   - A flat list of all sessions is persisted under a single
 *     kv key (`chat_sessions_v1`) via `kvGet/kvSet`.
 *   - The currently-active session's id is stored under
 *     `chat_active_session_v1` so the next launch re-opens
 *     the same conversation.
 *
 * Why this is a separate module:
 *   - Pure functions only (no React, no I/O) → unit-testable
 *     with vitest, including the "corrupt kv" boundary cases
 *     that bite the user when the SQLite file gets truncated.
 *   - The ChatZone component just wires these into React state.
 *
 * Storage backend note:
 *   The desktop app stores data in the OS app-data dir via the
 *   Tauri SQLite `kv` table. The frontend reaches it through
 *   `kvGet`/`kvSet` (see `apps/desktop/src/lib/api.ts`). We
 *   also fall back to `localStorage` if the kv call fails —
 *   that fallback is exercised in the test suite.
 */
import type { ChatTurnMessage } from '../lib/api.js';

// ── Types ───────────────────────────────────────────────────────

export interface ChatSession {
  /** Stable id (uuid v4-ish). Used as React key + kv sub-key. */
  id: string;
  /** Auto-derived from the first user turn (truncated to 40 chars). */
  title: string;
  /** When the session was first created. ISO 8601. */
  createdAt: string;
  /** When the session last received a message. ISO 8601. */
  updatedAt: string;
  /** All turns in the session (user + assistant + tool). */
  messages: ChatTurnMessage[];
  /** Mode the session was created in: workflow or chat. */
  mode: 'workflow' | 'chat';
}

// ── Storage keys ────────────────────────────────────────────────

export const STORAGE_KEY_SESSIONS = 'chat_sessions_v1';
export const STORAGE_KEY_ACTIVE = 'chat_active_session_v1';

// ── Pure helpers (no I/O) ───────────────────────────────────────

/** Generate a stable session id. Uses crypto.randomUUID when
 *  available (Tauri 2 webview always has it), falls back to a
 *  timestamp + random hex for the rare case it doesn't. */
export function newSessionId(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID();
  }
  return `s_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 10)}`;
}

/** Derive a title from the first user turn. Empty string → '新对话'. */
export function deriveTitle(firstUserContent: string): string {
  const trimmed = firstUserContent.trim();
  if (trimmed.length === 0) return '新对话';
  if (trimmed.length <= 40) return trimmed;
  return trimmed.slice(0, 40) + '…';
}

/** Build a fresh session from the first user turn. */
export function newSession(
  firstUserContent: string,
  mode: 'workflow' | 'chat',
): ChatSession {
  const now = new Date().toISOString();
  return {
    id: newSessionId(),
    title: deriveTitle(firstUserContent),
    createdAt: now,
    updatedAt: now,
    messages: [],
    mode,
  };
}

/** Sort sessions by `updatedAt` DESC (most recent first). */
export function sortByUpdatedAt(sessions: ChatSession[]): ChatSession[] {
  return [...sessions].sort((a, b) => b.updatedAt.localeCompare(a.updatedAt));
}

/** Cap a session's message array at `maxMessages`. Drops oldest. */
export function capMessages(
  session: ChatSession,
  maxMessages: number,
): ChatSession {
  if (session.messages.length <= maxMessages) return session;
  return {
    ...session,
    messages: session.messages.slice(-maxMessages),
  };
}

// ── Index ops (immutable; the caller persists the result) ───────

/** Append or insert a session by id. New = prepended (top of list). */
export function upsertSession(
  sessions: ChatSession[],
  next: ChatSession,
): ChatSession[] {
  const idx = sessions.findIndex((s) => s.id === next.id);
  if (idx === -1) {
    return [next, ...sessions];
  }
  const out = sessions.slice();
  out[idx] = next;
  return out;
}

/** Remove a session by id. Returns the updated list. */
export function removeSession(sessions: ChatSession[], id: string): ChatSession[] {
  return sessions.filter((s) => s.id !== id);
}

/** Find a session by id. */
export function findSession(
  sessions: ChatSession[],
  id: string,
): ChatSession | undefined {
  return sessions.find((s) => s.id === id);
}

/** Update `updatedAt` on a session (touch). */
export function touchSession(session: ChatSession): ChatSession {
  return { ...session, updatedAt: new Date().toISOString() };
}

// ── Boundary-safe loaders ───────────────────────────────────────

/**
 * Parse the raw value retrieved from `kvGet(STORAGE_KEY_SESSIONS)`.
 * Returns an empty list on ANY of:
 *   - `null` / `undefined` (key never written)
 *   - not an array
 *   - JSON shape doesn't match `ChatSession[]`
 *
 * This is the corrupt-storage boundary: if the user downgrades
 * the app, the SQLite kv table could have a partially-written
 * blob. We must NEVER crash on read — just drop and start over.
 */
export function loadSessions(raw: unknown): ChatSession[] {
  if (raw == null) return [];
  if (!Array.isArray(raw)) return [];
  const out: ChatSession[] = [];
  for (const entry of raw) {
    if (entry == null || typeof entry !== 'object') continue;
    const s = entry as Record<string, unknown>;
    if (
      typeof s.id !== 'string'
      || typeof s.title !== 'string'
      || typeof s.createdAt !== 'string'
      || typeof s.updatedAt !== 'string'
      || !Array.isArray(s.messages)
      || (s.mode !== 'workflow' && s.mode !== 'chat')
    ) {
      // Skip malformed entry but keep the rest.
      continue;
    }
    out.push({
      id: s.id,
      title: s.title,
      createdAt: s.createdAt,
      updatedAt: s.updatedAt,
      mode: s.mode,
      // Coerce messages through a tolerant filter; drop
      // individual bad messages but keep the session.
      messages: (s.messages as unknown[]).filter((m) => {
        if (m == null || typeof m !== 'object') return false;
        const mm = m as Record<string, unknown>;
        return typeof mm.role === 'string' && typeof mm.content === 'string';
      }) as ChatTurnMessage[],
    });
  }
  return out;
}

/**
 * Read the active session id from raw kv value. Returns null
 * if absent, not a string, or empty.
 */
export function loadActiveId(raw: unknown): string | null {
  if (typeof raw !== 'string') return null;
  if (raw.length === 0) return null;
  return raw;
}
