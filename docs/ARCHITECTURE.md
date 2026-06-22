# Architecture

> End-to-end architecture of Agent Company OS (v0.3+)

**Version:** v0.3 RFC
**Status:** Active
**Author:** Thatgfsj
**Related:** [TECH_STACK.md](./TECH_STACK.md) · [AGENT_PROTOCOL.md](./AGENT_PROTOCOL.md)
**Last updated:** 2026-06-22

---

## 1. Goals

1. Make the **whole-system data flow** drawable on one A3 page.
2. Pin the **module boundaries** so teams can own crates/packages in
   parallel without colliding.
3. Make every cross-module call **typed** and **versioned** — never
   `serde_json::Value` across crates without a contract.
4. **No Python in the runtime path.** Everything agent-related is Rust.

---

## 2. One-Page Diagram (v0.3)

```
                          ┌────────────────────────────────┐
                          │   Tauri Webview (System)       │
                          │                                │
                          │   React 19 + TS + Tailwind v4  │
                          │   ┌────────────────────────┐   │
                          │   │  IDE-style UI           │   │
                          │   │  文件树 │ Monaco │ Timeline │   │
                          │   │       │  diff  │          │   │
                          │   │       │ xterm.js│  Chat   │   │
                          │   └────────────┬───────────┘   │
                          │                │ Tauri IPC     │
                          └────────────────┼───────────────┘
                                           │
              ┌────────────────────────────┼────────────────────────────┐
              │   Tauri Backend (Rust, single process)                  │
              │                            │                            │
              │   crates/tauri-core        │   crates/agent-core        │
              │     commands, menu, tray   │     agent loop, context    │
              │     Tauri events  ◀────────┤     tool trait + registry  │
              │                            │     provider trait + impls │
              │   crates/event-bus         │     prompt engine          │
              │     in-process pub/sub     │     bash/read/write/patch  │
              │                            │     unified SSE streaming  │
              │   crates/storage            │                            │
              │     SQLx (SQLite + FTS5)    │   crates/provider-presets  │
              │     workflows, usage       │     built-in providers     │
              │                            │     (OpenAI/Anthropic/...) │
              │   crates/pipe-server       │                            │
              │     \\.\pipe\aco_runtime    │                            │
              │     JSON-RPC + events      │                            │
              │     (optional external API) │                            │
              │                            │                            │
              │   crates/config            │                            │
              │     providers.toml, ...    │                            │
              └────────────────────────────┴────────────────────────────┘
                                           │
                                           │ HTTPS (provider API calls)
                                           ▼
                              ┌────────────────────────┐
                              │  LLM Providers          │
                              │  OpenAI · Anthropic ·    │
                              │  Google · DeepSeek · ... │
                              │  (any OpenAI-compat)     │
                              └────────────────────────┘
```

**One process tree (v0.3):**

| Runtime | Language | Role |
|---------|----------|------|
| Tauri webview | TS/React | UI only |
| Tauri core + agent-core | Rust | IPC, FS, SQLite, agent loop, provider calls, event bus, pipe server |

**No Python. No external CLI.** The only outbound network calls go
directly from `agent-core` to LLM provider APIs over HTTPS.

---

## 3. Module Boundaries (Rust side)

```
crates/
├── tauri-core/        # Tauri app glue; commands, menu, tray, window
├── event-bus/         # in-process pub/sub; events flow Rust ⇄ webview
├── agent-core/        # ⭐ v0.3 — agent loop, tools, providers, context
│   ├── loop.rs
│   ├── context.rs
│   ├── tools/
│   ├── providers/
│   └── prompt/
├── provider-presets/  # ⭐ v0.3 — built-in provider catalog (data only)
├── pipe-server/       # ⭐ v0.3 — Rust named-pipe server (was Python)
├── config/            # providers.toml, aco.toml parsing
├── storage/           # SQLx repositories (workflows, usage, sessions)
└── shared/            # cross-crate types (events, errors, IPC)
```

**Rules:**

* `tauri-core` is the only crate that depends on `tauri`. All others
  are library-only and unit-testable in isolation.
* `event-bus` has no deps except `serde`, `tokio`, `thiserror`.
* `storage` is the only crate that talks to SQLite directly; everyone
  else goes through its `Repository` trait.
* `agent-core` owns all LLM calls; nothing else spawns HTTP requests
  to providers.
* The `bash` tool inside `agent-core/tools/bash.rs` is the only place
  that spawns processes; every invocation is logged and surfaced to
  the UI in real time.
* No Python in this tree. The `apps/runtime/` and `runtime/` Python
  packages are **deleted** as of v0.3.

---

## 4. agent-core Deep Dive

```
crates/agent-core/src/
├── lib.rs              public API: Agent::run(task) -> Stream<AgentEvent>
├── loop.rs             main loop: stream LLM → tool_calls → execute → repeat
├── context.rs          token counting + truncation + summarization
├── events.rs           AgentEvent enum (TextDelta, ToolStarted, ToolFinished, ...)
├── tools/
│   ├── mod.rs          Tool trait + ToolRegistry
│   ├── read.rs         cat a file with line range
│   ├── write.rs        atomic write + .bak before any change
│   ├── patch.rs        apply unified diff (bidirectional fuzzy match)
│   ├── bash.rs         tokio::process with timeout + live stdout
│   ├── grep.rs         ripgrep wrapper
│   └── glob.rs         glob pattern matcher
├── providers/
│   ├── mod.rs          Provider trait: stream_chat(messages) -> Stream<Chunk>
│   ├── openai.rs       OpenAI-compat (also covers DeepSeek, Moonshot, custom)
│   ├── anthropic.rs    Anthropic messages API + SSE
│   ├── google.rs       Gemini
│   └── custom.rs       user-defined relay (from provider-presets)
├── prompt/
│   ├── system.rs       system prompts per role (首席 / 缺陷猎手 / 工匠 / ...)
│   └── template.rs     placeholder substitution + context injection
└── tests/              unit tests per module + e2e mock provider tests
```

### 4.1 Agent loop (sketch)

```rust
pub async fn run(self, task: TaskEnvelope) -> Result<()> {
    let mut history = self.build_initial_messages(&task);

    loop {
        let resp = self.provider.stream_chat(&history).await?;
        let mut tool_calls = Vec::new();
        let mut text_buf = String::new();

        while let Some(chunk) = resp.next().await {
            match chunk? {
                Chunk::Text(s) => {
                    text_buf.push_str(&s);
                    self.bus.emit(AgentEvent::TextDelta {
                        agent: self.role,
                        delta: s,
                    });
                }
                Chunk::ToolUse(tc) => tool_calls.push(tc),
            }
        }

        history.push(Message::Assistant(
            text_buf.clone(),
            tool_calls.iter().map(|t| t.id.clone()).collect(),
        ));

        if tool_calls.is_empty() { break; }

        for tc in tool_calls {
            self.bus.emit(AgentEvent::ToolStarted {
                agent: self.role, name: tc.name.clone(), args: tc.args.clone(),
            });
            let result = self.tools.execute(&tc.name, tc.args, &self.workspace).await?;
            self.bus.emit(AgentEvent::ToolFinished {
                agent: self.role, name: tc.name.clone(), result: result.preview(),
            });
            history.push(Message::ToolResult {
                tool_call_id: tc.id, content: result.full(),
            });
        }

        if history.token_count() > self.context_budget {
            history = self.context.compact(history).await?;
        }
    }
    Ok(())
}
```

### 4.2 Tool trait

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn schema(&self) -> serde_json::Value;
    async fn execute(&self, args: serde_json::Value, ws: &Workspace)
        -> Result<ToolOutput>;
}
```

### 4.3 Patch algorithm

The `patch` tool accepts a unified diff:

1. **Bidirectional fuzzy match** — try forward, then reverse.
2. **Context-line trim** — tolerate whitespace drift.
3. **Atomic apply** — temp file + `fsync` + `rename(2)`. Original
   becomes `<file>.bak`.
4. **Rollback** — on error, restore `.bak`.

---

## 5. Frontend Layout

```
apps/desktop/src/
├── zones/              (legacy zone layout, kept for Settings & Plugins)
├── pages/              (v0.3 — new IDE-style single page)
│   ├── WorkspacePage.tsx       ⭐ the new IDE shell
│   ├── FileTreePanel.tsx
│   ├── MonacoDiffPanel.tsx
│   ├── TimelinePanel.tsx
│   ├── ConsolePanel.tsx
│   └── ChatDock.tsx
├── hooks/
│   ├── useEventStream.ts        (already exists)
│   ├── useAgentStream.ts        ⭐ text delta subscription
│   ├── useToolEvents.ts         ⭐ tool started/finished subscription
│   └── useFileTree.ts           ⭐ live file tree (notify-rs backend)
├── stores/             (Zustand)
└── lib/api.ts
```

---

## 6. Cross-Language Type Safety

Only TS ⇄ Rust now (no Python).

```
packages/shared/src/events.ts         ← source of truth
        │
        ▼
crates/shared/src/events.rs           (hand-mirrored, checked by CI)
```

---

## 7. Data Flow: One Workflow Run

```
User           UI          agent-core       Provider       SQLite
 │              │                │              │             │
 │── 任务 ────▶│                │              │             │
 │              │── invoke ────▶│              │             │
 │              │                │── stream ──▶│             │
 │              │                │◀─ tokens ───│             │
 │              │◀── text delta ─│              │             │
 │              │ (render live)  │              │             │
 │              │                │── tool_call: read foo.rs   │
 │              │                │── emit ToolStarted         │
 │              │                │── fs.read()                │
 │              │                │── emit ToolFinished        │
 │              │                │── stream ──▶│             │
 │              │                │◀─ tokens ───│             │
 │              │                │── tool_call: patch foo.rs  │
 │              │                │── atomic write + .bak      │
 │              │                │── emit ToolFinished        │
 │              │                │── stream ──▶│             │
 │              │                │◀─ tokens ───│             │
 │              │                │── no tool_calls → done     │
 │              │                │── save run ──────────────▶│
 │◀── done ─────│                │              │             │
```

---

## 8. Concurrency Model

* **One workflow = one 首席.** 工匠 / 缺陷猎手 / 质检师 are async
  tasks spawned by the 首席, not threads.
* **Tauri webview** runs on the main thread; React state updates go
  through TanStack Query → event-bus → Tauri IPC.
* **agent-core** uses tokio multi-thread. Each provider call is a
  cancellable `tokio::select!`.
* **SQLite** uses WAL mode. One writer, many readers.
* **SSE streams** are per-call `Stream` impls, not shared; cancel on
  agent abort.

---

## 9. Failure Domains

| Failure | Boundary that catches it | Recovery |
|---------|--------------------------|----------|
| Model API timeout | `agent-core/providers/*` | Router failover |
| 工匠 crash mid-task | 首席 via heartbeat timeout | REPAIR or ABORT |
| Provider 5xx / 429 | `agent-core/providers/*` | Backoff + retry, then failover |
| SQLite corruption | `storage` integrity check | Backup restore |
| Tauri webview crash | Tauri main process | Auto-reload webview |
| Whole-process crash | OS | Reopen app, scan workflows/*.jsonl, offer resume |

---

## 10. Trust Boundaries

```
   ┌────────────┐         ┌────────────┐         ┌────────────┐
   │  User      │         │  Plugins   │         │  Providers │
   │  (trusted) │         │ (sandboxed)│         │  (network) │
   └─────┬──────┘         └─────┬──────┘         └─────┬──────┘
         │                      │                      │
         ▼                      ▼                      ▼
   ┌────────────────────────────────────────────────────────────┐
   │  ACO Core (always-trusted)                                 │
   │  - Workflow state                                          │
   │  - SQLite                                                 │
   │  - File system                                            │
   │  - Tool execution (patch / bash)                          │
   └────────────────────────────────────────────────────────────┘
```

* The **user** trusts the core to do what they asked.
* The **core** does **not** trust plugins or providers.
* **Providers** are explicitly untrusted; never trust a model's
  output as code or SQL — it's just text until agent-core
  validates it.

---

## 11. Open Questions

1. Ship a Rust `aco doctor` CLI before v0.4? (proposed: yes)
2. Should `bash` tool require approval per invocation, or only for
   "dangerous" patterns? (proposed: dangerous patterns only)
3. Open-source the agent-core prompts? (proposed: yes, under
   `crates/agent-core/prompts/`)

---

**RFC ends.**