/**
 * v0.4.22 (event 000118, fix 6 hardening, persistence):
 * boundary tests for chatSessions.ts. The whole point of
 * extracting this into a pure module is so we can drive every
 * edge case from vitest without Tauri/localStorage.
 */
import { describe, expect, it } from "vitest";
import {
  appendChatTurn,
  appendMessageToSession,
  capMessages,
  deriveTitle,
  findSession,
  loadActiveId,
  loadSessions,
  newSession,
  newSessionId,
  removeSession,
  shouldMirrorToActiveSession,
  sortByUpdatedAt,
  STORAGE_KEY_ACTIVE,
  STORAGE_KEY_SESSIONS,
  touchSession,
  upsertSession,
  type ChatSession,
} from "./chatSessions.js";
import type { ChatTurnMessage } from "../lib/api.js";

const makeMsg = (role: ChatTurnMessage["role"], content: string): ChatTurnMessage => ({
  role,
  content,
});

const sampleSession = (overrides: Partial<ChatSession> = {}): ChatSession => ({
  id: "s_1",
  title: "demo",
  createdAt: "2026-01-01T00:00:00.000Z",
  updatedAt: "2026-01-01T00:00:00.000Z",
  messages: [],
  mode: "chat",
  ...overrides,
});

describe("STORAGE_KEY_*", () => {
  it("uses versioned keys (so a future schema break won\u2019t corrupt old data)", () => {
    expect(STORAGE_KEY_SESSIONS).toBe("chat_sessions_v1");
    expect(STORAGE_KEY_ACTIVE).toBe("chat_active_session_v1");
  });
});

describe("newSessionId", () => {
  it("generates non-empty ids", () => {
    const a = newSessionId();
    const b = newSessionId();
    expect(a.length).toBeGreaterThan(0);
    expect(b.length).toBeGreaterThan(0);
    expect(a).not.toBe(b);
  });

  it("generates 1000 ids with no collisions", () => {
    const ids = new Set<string>();
    for (let i = 0; i < 1000; i++) {
      ids.add(newSessionId());
    }
    expect(ids.size).toBe(1000);
  });
});

describe("deriveTitle", () => {
  it('returns "新对话" for empty / whitespace input', () => {
    expect(deriveTitle("")).toBe("新对话");
    expect(deriveTitle("   ")).toBe("新对话");
    expect(deriveTitle("\n\t  \n")).toBe("新对话");
  });

  it("keeps short input verbatim", () => {
    expect(deriveTitle("hello")).toBe("hello");
    expect(deriveTitle("a".repeat(40))).toBe("a".repeat(40));
  });

  it("truncates input > 40 chars with ellipsis", () => {
    const long = "a".repeat(100);
    const t = deriveTitle(long);
    expect(t.endsWith("…")).toBe(true);
    expect(t.length).toBe(41);
  });

  it("trims surrounding whitespace before measuring", () => {
    expect(deriveTitle("   hello   ")).toBe("hello");
  });
});

describe("newSession", () => {
  it("produces a session with the derived title and a fresh id", () => {
    const s = newSession("hi", "chat");
    expect(s.title).toBe("hi");
    expect(s.messages).toEqual([]);
    expect(s.mode).toBe("chat");
    expect(s.id.length).toBeGreaterThan(0);
    expect(s.createdAt).toBe(s.updatedAt);
  });

  it("records the mode so we can tell workflow from chat sessions later", () => {
    expect(newSession("x", "workflow").mode).toBe("workflow");
    expect(newSession("x", "chat").mode).toBe("chat");
  });
});

describe("sortByUpdatedAt", () => {
  it("sorts most-recently-updated first", () => {
    const a = sampleSession({ id: "a", updatedAt: "2026-01-01T00:00:00.000Z" });
    const b = sampleSession({ id: "b", updatedAt: "2026-06-01T00:00:00.000Z" });
    const c = sampleSession({ id: "c", updatedAt: "2026-03-01T00:00:00.000Z" });
    const sorted = sortByUpdatedAt([a, b, c]);
    expect(sorted.map((s) => s.id)).toEqual(["b", "c", "a"]);
  });

  it("does not mutate the input", () => {
    const a = sampleSession({ id: "a", updatedAt: "2026-01-01T00:00:00.000Z" });
    const b = sampleSession({ id: "b", updatedAt: "2026-06-01T00:00:00.000Z" });
    const before = [a, b];
    sortByUpdatedAt(before);
    expect(before).toEqual([a, b]);
  });

  it("handles empty list", () => {
    expect(sortByUpdatedAt([])).toEqual([]);
  });
});

describe("capMessages", () => {
  it("keeps all messages when under the cap", () => {
    const s = sampleSession({
      messages: [makeMsg("user", "a"), makeMsg("assistant", "b")],
    });
    const out = capMessages(s, 10);
    expect(out.messages.length).toBe(2);
  });

  it("drops oldest messages when over the cap", () => {
    const msgs = Array.from({ length: 100 }, (_, i) => makeMsg("user", `m_${i}`));
    const s = sampleSession({ messages: msgs });
    const out = capMessages(s, 12);
    expect(out.messages.length).toBe(12);
    // Oldest 88 dropped; newest 12 kept.
    expect(out.messages[0]?.content).toBe("m_88");
    expect(out.messages[11]?.content).toBe("m_99");
  });

  it("returns the same object when no capping needed (immutability guard)", () => {
    const s = sampleSession({ messages: [makeMsg("user", "a")] });
    expect(capMessages(s, 10)).toBe(s);
  });
});

describe("upsertSession", () => {
  it("prepends a new session", () => {
    const list = [sampleSession({ id: "old" })];
    const next = sampleSession({ id: "new" });
    const out = upsertSession(list, next);
    expect(out[0]?.id).toBe("new");
    expect(out.length).toBe(2);
  });

  it("replaces an existing session in place", () => {
    const list = [
      sampleSession({ id: "a" }),
      sampleSession({ id: "b" }),
      sampleSession({ id: "c" }),
    ];
    const updated = sampleSession({ id: "b", title: "updated" });
    const out = upsertSession(list, updated);
    expect(out.length).toBe(3);
    expect(out.find((s) => s.id === "b")?.title).toBe("updated");
  });

  it("does not mutate the input", () => {
    const list = [sampleSession({ id: "a" })];
    upsertSession(list, sampleSession({ id: "b" }));
    expect(list.length).toBe(1);
  });
});

describe("removeSession", () => {
  it("removes by id", () => {
    const out = removeSession([sampleSession({ id: "a" }), sampleSession({ id: "b" })], "a");
    expect(out.map((s) => s.id)).toEqual(["b"]);
  });

  it("no-op when id missing", () => {
    const list = [sampleSession({ id: "a" })];
    expect(removeSession(list, "zzz")).toEqual(list);
  });
});

describe("findSession", () => {
  it("finds by id", () => {
    const list = [sampleSession({ id: "a" }), sampleSession({ id: "b" })];
    expect(findSession(list, "b")?.id).toBe("b");
    expect(findSession(list, "zzz")).toBeUndefined();
  });
});

describe("touchSession", () => {
  it("bumps updatedAt to a later value", async () => {
    const s = sampleSession({ updatedAt: "2026-01-01T00:00:00.000Z" });
    // Sleep a millisecond so the ISO string is guaranteed different.
    await new Promise((r) => setTimeout(r, 5));
    const t = touchSession(s);
    expect(t.updatedAt > s.updatedAt).toBe(true);
    expect(t.createdAt).toBe(s.createdAt);
  });
});

// ── loadSessions: corrupt-storage boundary cases ───────────────

describe("loadSessions — corrupt storage resilience", () => {
  it("null → empty list", () => {
    expect(loadSessions(null)).toEqual([]);
  });

  it("undefined → empty list", () => {
    expect(loadSessions(undefined)).toEqual([]);
  });

  it("not an array → empty list", () => {
    expect(loadSessions({ id: "x" })).toEqual([]);
    expect(loadSessions("oops")).toEqual([]);
    expect(loadSessions(42)).toEqual([]);
  });

  it("empty array → empty list", () => {
    expect(loadSessions([])).toEqual([]);
  });

  it("drops malformed entries but keeps good ones", () => {
    const good = sampleSession({ id: "good" });
    const raw = [
      good,
      { id: "missing-title" },
      { title: "no id" },
      null,
      "not an object",
      { id: "", title: "empty id" },
    ];
    const out = loadSessions(raw);
    expect(out.length).toBe(1);
    expect(out[0]?.id).toBe("good");
  });

  it("drops messages with wrong shape but keeps the session", () => {
    const raw = [
      {
        ...sampleSession({ id: "mixed" }),
        messages: [
          { role: "user", content: "ok" },
          { role: "user" }, // missing content
          null,
          "not an object",
          { role: "user", content: "also ok" },
        ],
      },
    ];
    const out = loadSessions(raw);
    expect(out.length).toBe(1);
    expect(out[0]?.messages.length).toBe(2);
  });

  it("preserves valid mode enum", () => {
    const raw = [
      sampleSession({ id: "a", mode: "chat" }),
      sampleSession({ id: "b", mode: "workflow" }),
    ];
    const out = loadSessions(raw);
    expect(out.find((s) => s.id === "a")?.mode).toBe("chat");
    expect(out.find((s) => s.id === "b")?.mode).toBe("workflow");
  });

  it("drops sessions with invalid mode", () => {
    const raw = [
      sampleSession({ id: "a", mode: "chat" }),
      sampleSession({ id: "b", mode: "borked" as unknown as "chat" }),
    ];
    const out = loadSessions(raw);
    expect(out.length).toBe(1);
    expect(out[0]?.id).toBe("a");
  });

  it("survives a JSON.parse-style corruption (string instead of object)", () => {
    // Imagine kvSet wrote a raw string by mistake.
    expect(loadSessions('{"foo":"bar"}')).toEqual([]);
  });
});

describe("loadActiveId", () => {
  it("null/undefined → null", () => {
    expect(loadActiveId(null)).toBeNull();
    expect(loadActiveId(undefined)).toBeNull();
  });

  it("non-string → null", () => {
    expect(loadActiveId(42)).toBeNull();
    expect(loadActiveId({ id: "x" })).toBeNull();
  });

  it("empty string → null", () => {
    expect(loadActiveId("")).toBeNull();
  });

  it("valid string → returned as-is", () => {
    expect(loadActiveId("s_abc")).toBe("s_abc");
  });
});

// ── End-to-end happy path: round-trip the persistence shape ───

describe("round-trip", () => {
  it("save → load preserves all fields", () => {
    const session = sampleSession({
      id: "s_42",
      title: "how to write tests",
      mode: "workflow",
      messages: [
        makeMsg("user", "how to write tests?"),
        makeMsg("assistant", "start with edge cases"),
      ],
    });
    const serialized = JSON.parse(JSON.stringify([session]));
    const out = loadSessions(serialized);
    expect(out.length).toBe(1);
    expect(out[0]).toEqual(session);
  });
});

// ── Send attribution (issue #26 defect 4) ───────────────────────
//
// A send awaits the backend before writing the user turn, and the
// assistant reply arrives even later. If the user switches sessions
// in between, the write must still go to the session that started
// the send — not to whatever is active when the await resolves.

describe("appendMessageToSession — origin-session attribution", () => {
  const originA = (): ChatSession =>
    sampleSession({
      id: "A",
      updatedAt: "2026-01-01T00:00:00.000Z",
      messages: [makeMsg("user", "hello")],
    });
  const otherB = (): ChatSession =>
    sampleSession({
      id: "B",
      updatedAt: "2026-01-01T00:00:00.000Z",
      messages: [makeMsg("user", "unrelated")],
    });

  it("writes the user turn to the originating session only", () => {
    // User is looking at B when the send from A resolves.
    const out = appendMessageToSession(
      [originA(), otherB()],
      "A",
      makeMsg("user", "sent from A"),
      12,
    );
    expect(findSession(out, "A")?.messages.map((m) => m.content)).toEqual(["hello", "sent from A"]);
    expect(findSession(out, "B")?.messages.map((m) => m.content)).toEqual(["unrelated"]);
  });

  it("keeps the assistant reply with the origin after a mid-stream switch", () => {
    const afterSwitch = appendMessageToSession(
      [originA(), otherB()],
      "A",
      makeMsg("assistant", "reply from A"),
      12,
    );
    expect(findSession(afterSwitch, "A")?.messages.map((m) => m.content)).toEqual([
      "hello",
      "reply from A",
    ]);
    expect(findSession(afterSwitch, "B")?.messages.map((m) => m.content)).toEqual(["unrelated"]);
    // The live chatHistory (now B's) must not mirror A's reply.
    expect(shouldMirrorToActiveSession("B", "A")).toBe(false);
  });

  it("does not touch the transcript of an unrelated session", () => {
    const before = [originA(), otherB()];
    const out = appendMessageToSession(before, "A", makeMsg("user", "x"), 12);
    expect(findSession(out, "B")).toEqual(findSession(before, "B"));
  });

  it("is a no-op when the originating session was deleted mid-send", () => {
    const before = [otherB()];
    expect(appendMessageToSession(before, "A", makeMsg("user", "x"), 12)).toBe(before);
  });

  it("caps the transcript and keeps the newest turns", () => {
    const full = sampleSession({
      id: "A",
      messages: Array.from({ length: 12 }, (_, i) => makeMsg("user", `m_${i}`)),
    });
    const out = appendMessageToSession([full], "A", makeMsg("user", "newest"), 12);
    const messages = findSession(out, "A")?.messages ?? [];
    expect(messages.length).toBe(12);
    expect(messages[0]?.content).toBe("m_1");
    expect(messages[11]?.content).toBe("newest");
  });

  it("does not mutate the input list or session objects", () => {
    const before = [originA(), otherB()];
    const snapshot = JSON.parse(JSON.stringify(before));
    appendMessageToSession(before, "A", makeMsg("user", "x"), 12);
    expect(before).toEqual(snapshot);
  });

  it("moves the touched session to the front of the list", () => {
    const out = appendMessageToSession([otherB(), originA()], "A", makeMsg("user", "bump"), 12);
    expect(out.map((s) => s.id)).toEqual(["A", "B"]);
  });
});

describe("shouldMirrorToActiveSession — live history gate", () => {
  it("mirrors only while the origin is still the active session", () => {
    expect(shouldMirrorToActiveSession("A", "A")).toBe(true);
    expect(shouldMirrorToActiveSession("B", "A")).toBe(false);
    expect(shouldMirrorToActiveSession(null, "A")).toBe(false);
  });
});

describe("appendChatTurn — live history cap", () => {
  it("appends and caps at maxMessages", () => {
    const history = Array.from({ length: 12 }, (_, i) => makeMsg("user", `m_${i}`));
    const out = appendChatTurn(history, makeMsg("assistant", "newest"), 12);
    expect(out.length).toBe(12);
    expect(out[11]?.content).toBe("newest");
    expect(out[0]?.content).toBe("m_1");
  });

  it("does not mutate the input array", () => {
    const history = [makeMsg("user", "a")];
    const out = appendChatTurn(history, makeMsg("assistant", "b"), 12);
    expect(history.length).toBe(1);
    expect(out.length).toBe(2);
  });
});
