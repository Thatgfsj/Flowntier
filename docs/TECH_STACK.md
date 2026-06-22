# Tech Stack

> Locked-in technology choices for Agent Company OS v0.3+

**Version:** v0.3 RFC
**Status:** Active
**Author:** Thatgfsj
**Last updated:** 2026-06-22

---

## 1. Goals

1. **Lock the stack early.** Future RFCs reference these choices; no
   scope creep.
2. **All Rust on the backend.** One language for IPC, FS, SQLite,
   agent loop, providers, tools, pipe server.
3. **Local-first.** Everything works offline. Cloud is a v1.0+ concern.
4. **Desktop-first, cross-platform.** Windows / macOS / Linux day one.
5. **Pluggable AI.** No SDK lock-in. New providers slot in via the
   `Provider` trait (see [PROVIDER_SPEC.md](./PROVIDER_SPEC.md)).

---

## 2. Architecture at a Glance

```
┌──────────────────────────────────────────────────────────────────┐
│                  Tauri Webview (System Webview)                  │
│                                                                  │
│   ┌────────────────────────────────────────────────────────────┐ │
│   │  React 19 + TypeScript + Tailwind v4 + shadcn/ui          │ │
│   │                                                            │ │
│   │  IDE-style WorkspacePage (v0.3)                            │ │
│   │  Files (Zustand) · Monaco · xterm.js · React Flow · Motion │ │
│   └────────────────────────┬───────────────────────────────────┘ │
│                            │ Tauri IPC (typed)                    │
│   ┌────────────────────────┴───────────────────────────────────┐ │
│   │  Rust Backend (Tokio) — SINGLE PROCESS                      │ │
│   │   tauri-core · event-bus · agent-core ⭐ · pipe-server ⭐ · │ │
│   │   provider-presets ⭐ · config · storage (SQLx) · shared   │ │
│   └────────────────────────────────────────────────────────────┘ │
│                              │                                    │
│                              ▼                                    │
│                    HTTPS to LLM provider APIs                     │
│                    (OpenAI / Anthropic / Google / ...)            │
└──────────────────────────────────────────────────────────────────┘
```

**Two runtimes, one process tree:**

| Runtime | Language | Role |
|---------|----------|------|
| Tauri webview | TS/React | UI only |
| Tauri backend (everything else) | Rust | IPC, FS, SQLite, agent loop, providers, tools, pipe server |

**v0.3 explicitly removes:** the Python runtime sidecar, the Claude
Code CLI sidecar, the portable-pty layer.

---

## 3. Desktop Shell

| Choice | Why |
|--------|-----|
| **Tauri v2** | Small binary, Rust core reuses our backend crates, no Electron. |
| **Rust** (≥ 1.85) | All backend code; same language end-to-end. |
| **React 19** | Ecosystem, hiring, streaming Suspense. |
| **TypeScript** (≥ 5.6) | Strict mode everywhere. |
| **Vite** | Tauri default; HMR is fast. |

**System webview:** WebView2 on Windows, WKWebView on macOS, WebKitGTK
on Linux. No bundled Chromium.

---

## 4. Frontend

| Library | Use |
|---------|-----|
| **React 19** | UI runtime |
| **TypeScript** | Types |
| **Tailwind CSS v4** | Styling; tokens from UI_GUIDELINES.md |
| **shadcn/ui** | Component primitives (Radix under the hood) |
| **Motion (Framer)** | Phase transitions, status changes |
| **Zustand** | Local UI state |
| **TanStack Query** | Server-state cache for workflow events |
| **React Router** | In-app navigation |
| **React Hook Form** | Settings forms |
| **Zod** | Runtime validation of Tauri IPC payloads |
| **Monaco Editor** | Read-only diff / source view in IDE page |
| **xterm.js** | Bottom console (bash tool output) |
| **React Flow** | Task graph visualization (首席's plan) |
| **Live2D Cubism** | Avatars — v0.5 only |

**State boundary:**

* **Server state** (events, task list, console): TanStack Query
* **Local UI state**: Zustand
* **Form state**: React Hook Form + Zod
* **No Redux.** Never.

---

## 5. Backend (Rust)

| Crate | Use |
|-------|-----|
| **Tokio** | Async runtime (multi-thread) |
| **Tauri v2** | Desktop shell + IPC |
| **reqwest** + **eventsource-stream** | HTTPS + SSE to LLM providers |
| **Serde** | (De)serialization |
| **sqlx** | Async SQLite |
| **SQLite** + FTS5 | Storage + full-text search |
| **notify** | FS watcher (live file tree) |
| **tokio::process** | `bash` tool subprocess (no portable-pty) |
| **similar** | Unified-diff apply (patch tool) |
| **ignore** / **globset** | gitignore-aware file matching |
| **ripgrep** (optional dep) | `grep` tool |
| **tracing** + **tracing-subscriber** | Structured logs |
| **thiserror** + **anyhow** | Error handling |
| **clap** (derive) | CLI subcommands (`aco run`, `aco doctor`) |
| **async-trait** | Async traits (Provider, Tool) |
| **ulid** + **chrono** | IDs and timestamps |

**Workspace layout (Rust side):**

```
crates/
├── tauri-core/        # Tauri app glue (commands, menus, tray)
├── event-bus/         # In-process pub/sub for Rust ↔ Tauri webview
├── agent-core/        # ⭐ v0.3 — agent loop, tools, providers, context
├── provider-presets/  # ⭐ v0.3 — built-in provider catalog (data only)
├── pipe-server/       # ⭐ v0.3 — Rust named-pipe server
├── config/            # providers.toml, router.toml parsing
├── storage/           # SQLx repositories, FTS5 index
└── shared/            # Cross-crate types (events, errors, IPC)
```

All crates share a Cargo workspace at the repo root.

---

## 6. (Removed) AI Runtime — Python

**Removed as of v0.3.** Previously:

* `runtime/` (Python uv workspace)
* `apps/runtime/` (FastAPI/uvicorn sidecar)
* `crates/claude-adapter/` (portable-pty wrapper around `claude` CLI)

**Why removed:**

* Cold-start cost (~400 ms Python import chain on every task)
* Two-language debugging surface (Pydantic ↔ serde ↔ Zod drift)
* Python's GIL limited concurrent SSE parsing
* PTY-spawn-via-CLI was a layer that added failure modes (CLI
  crash, JSON-extraction fragility, non-streamable progress)

**Migration:** all agent logic ported to `crates/agent-core/`. All
provider HTTP calls ported to `crates/agent-core/src/providers/`.
All tool implementations ported to `crates/agent-core/src/tools/`.

---

## 7. Agent Framework

Built **in-house** in Rust on top of `tokio`:

* **Agent loop** (`agent-core/src/loop.rs`) — implements the
  stream-LLM → tool_calls → execute → repeat pattern.
* **Event Bus integration** — pub/sub via `event-bus` so every
  step streams to the UI in real time.
* **Task Graph Scheduler** — dispatches per-task envelopes from
  the 首席's plan.
* **Prompt Engine** — renders prompt templates with task payload
  + role-specific system prompt.
* **Model Router** — see [PROVIDER_SPEC.md](./PROVIDER_SPEC.md).
* **Provider Manager** — health, failover, cost tracking, all in
  Rust.

**Why custom, not LangChain / AutoGen / OpenCode SDK?**

We need **strict isolation** guarantees (workers never talk to each
other, no peer channels). Off-the-shelf frameworks leak context
across agents. Also: OpenCode is a separate TUI product with its
own session model; integrating it as a library is more work than
writing our own 200-line loop.

---

## 8. AI SDKs → HTTP APIs

| Provider | Used for | Rust impl |
|----------|----------|-----------|
| **OpenAI** | GPT-4o, GPT-4-Turbo, o1, o3 | `reqwest` + SSE |
| **Anthropic** | Claude 3.5/3.7/4 Sonnet, Opus, Haiku | `reqwest` + SSE |
| **Google Gemini** | Gemini 2.0/2.5 Pro/Flash | `reqwest` + SSE |
| **Moonshot** | Kimi K2 | OpenAI-compat (covered by `openai.rs`) |
| **DeepSeek** | DeepSeek Chat, Reasoner | OpenAI-compat |
| **Ollama** | Local models | OpenAI-compat |
| **LM Studio** | Local models | OpenAI-compat |
| **Custom relay** | User-defined endpoints | OpenAI-compat |
| **OpenRouter** | Aggregator | OpenAI-compat |

All providers go through the `Provider` trait. A new provider is
**one file** in `crates/agent-core/src/providers/`.

**v0.3 removes** from the built-in preset catalog: MiniMax, DeepSeek
(users can still add them as custom relays).

---

## 9. (Removed) CLI Integration

**Removed as of v0.3.** Previously:

| CLI | Status |
|-----|--------|
| Claude Code | shipped (portable-pty) |

Replaced by direct HTTPS calls + in-process tool execution.

---

## 10. Database

* **SQLite** (single file, WAL mode, per-user)
* **SQLx** for async Rust access
* **FTS5** virtual table for full-text search across:
  * Workflow logs
  * Console lines
  * Prompt history
  * Plugin contribution index

Path: `$APPDATA/aco/storage.sqlite` (Windows) /
`~/Library/Application Support/aco/storage.sqlite` (macOS) /
`~/.config/aco/storage.sqlite` (Linux).

---

## 11. Configuration

| Format | Used for |
|--------|----------|
| **TOML** | `providers.toml`, `router.toml`, `aco.toml` |
| **YAML** | Project-level `.aco/config.yaml` |
| **dotenv** | Local dev overrides (`.env`, gitignored) |

**Hierarchy** (later overrides earlier):

1. Built-in defaults
2. `~/.config/aco/config.toml` (user-global)
3. `<project>/.aco/config.yaml` (project)
4. `<project>/.env` (gitignored local)
5. Environment variables (highest)

**API keys are env-var only.** Never in any TOML/YAML.

---

## 12. Storage

```
$ACO_DATA/
├── storage.sqlite
├── storage.sqlite-wal
├── workflows/<wf_id>.jsonl
├── usage/<yyyy-mm>.jsonl       # append-only
├── cache/
│   ├── prompts/<role>/<version>.json
│   └── models/<provider>.json
└── plugins/                    # user-installed plugins
```

---

## 13. Communication

| Channel | Purpose |
|---------|---------|
| **Tauri IPC** (typed) | Rust ⇄ React |
| **Event Bus** (Rust internal) | Cross-crate pub/sub |
| **Named pipes** (Windows) | `\\.\pipe\aco_runtime` + `\\.\pipe\aco_runtime_events` for external integrations |
| **HTTPS** | agent-core ⇄ provider APIs |

The named pipes survive from v0.2 for backward compat with external
tools; the desktop app itself talks to agent-core through Tauri IPC
and the event bus directly.

---

## 14. Logging

* **Rust:** `tracing` + `tracing-subscriber` (JSON in prod, pretty in dev)
* **Tauri webview:** console logs piped to Rust via `tauri-plugin-log`

Log levels, sampling, and redaction live in `aco.toml`:

```toml
[logging]
level = "info"
redact = ["*KEY*", "*TOKEN*", "*SECRET*"]
sample.console = 1.0
sample.events = 0.1
```

---

## 15. Plugin System

| Surface | Status | Spec |
|---------|--------|------|
| **WASM Plugin API** | reserved v0.4 | PLUGIN_SPEC §7 |
| **MCP Support** | v0.4 | A plugin that bridges MCP servers |
| **Git Plugin** | built-in agent tool | Uses `tokio::process git` |
| **Docker Plugin** | v0.4 | |
| **Browser Plugin** | v0.4 | Playwright-style web testing |

**v0.3 plugins are agent tools.** They live under
`crates/agent-core/src/tools/<name>.rs` and implement the `Tool`
trait. No external plugin loading yet — that's v0.4.

---

## 16. Visualization

| Tool | Use |
|------|-----|
| **Mermaid** | Plan doc as a graph (legacy) |
| **React Flow** | Interactive task graph (首席's plan) |
| **Monaco** | Source / diff viewer in IDE page |
| **Motion** | Phase transitions, status pulses |
| **Live2D** | Avatars (v0.5) |

---

## 17. Testing

### Frontend
* **Vitest** — unit + component tests
* **Playwright** — E2E (smoke flows)

### Backend (Rust)
* **cargo test** (workspace-wide)
* **insta** for snapshot tests of serialized events
* **mockall** for trait mocks (Provider, Tool)
* **wiremock** for provider HTTP mocks

### Cross-language
* **Snapshot tests** in `crates/agent-core/tests/`:
  1. Define a JSON fixture of a full agent run (input messages
     + expected events).
  2. Run it through agent-core with a mock provider.
  3. Assert the final event log matches the snapshot.

### Targets

| Layer | Coverage target |
|-------|-----------------|
| Rust core | ≥ 80% |
| React UI | ≥ 60% |

---

## 18. Packaging

* **Tauri Bundle** produces:
  * Windows: `.msi` (WiX) + `.exe` (NSIS)
  * macOS: `.dmg` + `.app`
  * Linux: `.deb` + `.rpm` + `.AppImage`
* **No PyInstaller / PyOxidizer** — v0.3 ships zero Python.
* Code signing:
  * Windows: EV cert (planned for v1.0)
  * macOS: Developer ID (planned for v1.0)
  * Linux: GPG signature on `.deb`/`.rpm`
* Auto-update: Tauri Updater plugin, opt-in (off by default in v0.3)

---

## 19. CI/CD

GitHub Actions on `.github/workflows/`:

| Workflow | Triggers | Steps |
|----------|----------|-------|
| `ci.yml` | PR, push to main | cargo test, vitest, tsc, lint, fmt |
| `lint.yml` | PR | clippy, rustfmt, eslint, prettier |
| `build.yml` | tag `v*` | tauri build (3 OSes in parallel) |
| `release.yml` | manual | Draft GitHub release with bundle artifacts |

**Linters / formatters (all required, no bypass):**

* **Rust:** clippy (`-D warnings`), rustfmt
* **TS:** eslint, prettier
* **Markdown:** prettier

**Branch protection on `main`:** require CI green + 1 review.

---

## 20. Monorepo Layout

```
AgentCompanyOS/
├── apps/
│   └── desktop/        # Tauri app (src-tauri + src/)
│                        # NO apps/runtime/ (Python removed)
│
├── packages/            # pnpm workspace (TS)
│   ├── ui/             # Shared React components
│   ├── workflow/       # Workflow state types (TS) + thin client
│   ├── providers/      # Provider metadata tables (TS, data only)
│   ├── prompts/        # Prompt template renderer (TS)
│   └── shared/         # Cross-language types: events, errors, IPC
│
├── crates/             # Cargo workspace
│   ├── tauri-core/
│   ├── event-bus/
│   ├── agent-core/     # ⭐ v0.3
│   ├── provider-presets/  # ⭐ v0.3
│   ├── pipe-server/    # ⭐ v0.3
│   ├── config/
│   ├── storage/
│   └── shared/
│
│   # NO runtime/  (Python removed)
│   # NO crates/claude-adapter/  (PTY CLI removed)
│
├── prompts/            # Prompt template files (mirrored into crates/agent-core)
├── assets/             # Avatars, icons
├── docs/               # RFCs (this directory)
├── plugins/            # Built-in plugin sources (future)
│
├── .github/workflows/
├── Cargo.toml          # Rust workspace root
├── pnpm-workspace.yaml # TS workspace root
├── aco.toml            # Top-level ACO config
└── README.md
```

### 20.1 Workspace tooling

| Layer | Tool | Top-level file |
|-------|------|----------------|
| TS | **pnpm** 9.x | `pnpm-workspace.yaml` |
| Rust | **Cargo** 1.85 | `Cargo.toml` (workspace) |

No Makefile needed — `cargo run -p aco-desktop` starts everything.

---

## 21. Inter-Language Contracts

The single biggest cross-language risk is **type drift** between
TypeScript and Rust.

1. **All IPC event types live in `packages/shared/`** as:
   * TypeScript: `packages/shared/src/events.ts` (source of truth)
   * Rust: hand-mirrored in `crates/shared/src/`
2. **CI runs a `types-sync` check** that diffs the two and fails on
   drift.

---

## 22. Out of Scope (v0.3)

* Cloud-hosted ACO
* Multi-user / multi-tenant
* Voice / Live2D
* WASM plugins (v0.4)
* Auto-update (planned but off by default)
* Code signing (planned for v1.0)
* Mobile / responsive < 768 px
* Python in any form

---

## 23. Open Questions

1. Should the `bash` tool require user approval for each invocation?
   (proposed: only for "dangerous" patterns — see
   `crates/agent-core/tools/bash.rs`)
2. Should we ship a Rust `aco doctor` CLI in v0.3? (proposed: yes)
3. Should we bundle a tiny Markdown formatter as a built-in tool?
   (proposed: yes — keeps agent runs self-contained)

---

**RFC ends.**