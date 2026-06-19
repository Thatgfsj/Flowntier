# Agent Company OS (ACO)

> A Visual AI Software Company Powered by Multi-Agent Workflow

[![Release](https://img.shields.io/badge/release-v0.2.2-blue)](https://github.com/Thatgfsj/AgentCompanyOS/releases/tag/v0.2.2)
[![License](https://img.shields.io/badge/license-MIT-green)]()
[![RFCs](https://img.shields.io/badge/RFCs-15-orange)]()
[![Tests](https://img.shields.io/badge/tests-153%20passing-brightgreen)]()
[![Windows](https://img.shields.io/badge/windows-installer-blueviolet)](https://github.com/Thatgfsj/AgentCompanyOS/releases/tag/v0.2.2)

**ACO is not another AI IDE. It is an AI Software Company Operating System.**

Users interact with a beautiful visual workspace while multiple AI agents
collaborate behind the scenes — exactly like a real software company:

```
User → Chief Agent → Planning → Critic Review → Workers → Review → Delivery
```

The IDE is only the visualization layer. **The workflow is the product.**

---

## 📚 Documentation

All design decisions live in versioned RFCs under [`docs/`](./docs/).

### RFCs — design contracts

| Document | Purpose |
|----------|---------|
| [PROJECT_SPEC.md](./PROJECT_SPEC.md) | Top-level product vision, philosophy, agents, workflow, roadmap |
| [docs/TECH_STACK.md](./docs/TECH_STACK.md) | Locked-in tech: Tauri + Rust + React 19 + Python runtime, monorepo layout |
| [docs/ARCHITECTURE.md](./docs/ARCHITECTURE.md) | End-to-end architecture: data flow, module boundaries, cross-language contracts |
| [docs/WORKFLOW_SPEC.md](./docs/WORKFLOW_SPEC.md) | 8-phase state machine, transitions, budgets, replay format |
| [docs/AGENT_PROTOCOL.md](./docs/AGENT_PROTOCOL.md) | Inter-agent message envelope, task lifecycle, isolation rules |
| [docs/PROVIDER_SPEC.md](./docs/PROVIDER_SPEC.md) | Multi-provider model layer (Anthropic / OpenAI / Gemini / etc.) |
| [docs/UI_GUIDELINES.md](./docs/UI_GUIDELINES.md) | Mission Control UI design system, layout, components, themes |
| [docs/PROMPT_GUIDE.md](./docs/PROMPT_GUIDE.md) | Per-agent prompt templates and authoring rules |
| [docs/PLUGIN_SPEC.md](./docs/PLUGIN_SPEC.md) | Plugin interface (Git / Docker / MCP / Browser / etc.) |
| [docs/STORAGE_SPEC.md](./docs/STORAGE_SPEC.md) | SQLite schema, FTS5, JSONL log, backup/recovery |
| [docs/TASK_GRAPH.md](./docs/TASK_GRAPH.md) | Plan DAG, scheduling, parallelism, repair sub-graphs |
| [docs/SECURITY.md](./docs/SECURITY.md) | Threat model, secrets, sandbox, audit |
| [docs/CONFIG.md](./docs/CONFIG.md) | Config hierarchy, schemas, validation |
| [docs/ROADMAP.md](./docs/ROADMAP.md) | Long-term versioning, milestones, deprecations |
| [docs/FAQ.md](./docs/FAQ.md) | Frequently asked questions |

### Prompts — runnable agent system prompts

In [`prompts/`](./prompts/):

| File | Role |
|------|------|
| [bootstrap.md](./prompts/bootstrap.md) | Initial system prompt for every agent |
| [chief_agent.md](./prompts/chief_agent.md) | Chief Agent (orchestrator) |
| [critic_a.md](./prompts/critic_a.md) | Critic A — bug hunter |
| [critic_b.md](./prompts/critic_b.md) | Critic B — architect |
| [worker.md](./prompts/worker.md) | Generic Worker |
| [planner.md](./prompts/planner.md) | Planner sub-role |
| [reporter.md](./prompts/reporter.md) | Reporter (final summary) |
| [merger.md](./prompts/merger.md) | Merger (combine parallel worker outputs) |

### Plans — phased implementation roadmap

In [`plans/`](./plans/):

| File | Phase |
|------|-------|
| [Phase0.md](./plans/Phase0.md) | Foundation: monorepo scaffold, CI, RFCs |
| [Phase1.md](./plans/Phase1.md) | Minimal runtime: state machine + Anthropic + 1 e2e test |
| [Phase2.md](./plans/Phase2.md) | Task graph + multi-provider failover + first 3 plugins |
| [Phase3.md](./plans/Phase3.md) | Memory + replay + cost dashboard + i18n |
| [Phase4.md](./plans/Phase4.md) | Real-world plugins + marketplace + house-style prompts |
| [Phase5.md](./plans/Phase5.md) | Live2D + voice (Whisper + Piper) + streaming |
| [ReleasePlan.md](./plans/ReleasePlan.md) | Releases, branching, support policy, dogfooding |

### Agent rules — what the runtime (and humans) must follow

In [`.agent/`](./.agent/):

| File | Scope |
|------|-------|
| [coding_rules.md](./.agent/coding_rules.md) | Languages, style, naming, errors, tests, deps |
| [ui_rules.md](./.agent/ui_rules.md) | React 19 + Tailwind v4 + shadcn/ui conventions |
| [commit_rules.md](./.agent/commit_rules.md) | Conventional commits + signing + PR flow |
| [architecture_rules.md](./.agent/architecture_rules.md) | Module deps, IPC, DB, schemas, forbidden patterns |

> **Convention:** Anything in `docs/` is a *Request For Comments* — proposals
> are reviewed before implementation. Once accepted, the RFC is the source of
> truth for that subsystem.

---

## 🚀 5-minute demo (no API key needed)

```bash
# 1. Install runtime + deps (already in apps/desktop/ if you cloned the
#    monorepo; otherwise pip install -e runtime).
pip install -e ./runtime

# 2. Run an end-to-end multi-agent workflow. Uses the deterministic
#    MockProvider so no LLM API key is required.
python -m aco_runtime_lib demo "Write an is_prime function with pytest tests"

# Expected output (abridged):
#   ACO runtime demo — end-to-end multi-agent workflow
#   Request: 'is_prime'
#
#   Result  state: DONE
#           tasks : 2
#           LLM   : 4 calls
#           time  : 0.01s
#
#   Delivery summary:
#   # Delivery Summary
#   ## What was built
#   - is_prime checks n>1 then trial-divides up to sqrt(n)
#   ...
```

The demo exercises the full state machine (Chief → Planner →
Critic → Worker ×2 → Reporter → FinalReviewer → DONE) using the
real orchestrator code. Swap the `MockProvider` for a real
provider (set `MINIMAX_API_KEY` in the OS keychain via the
Settings UI) to see the same flow with real LLM calls.

---

## 🏗️ Status

**Current version:** [`v0.2.2`](https://github.com/Thatgfsj/AgentCompanyOS/releases/tag/v0.2.2)
(Phase 2 partial: parser + validator + scheduler shipped; full Phase 2 in progress.)

| Milestone | Status |
|-----------|--------|
| Phase 0 — Foundation (monorepo, CI, RFCs, Windows installer) | ✅ Done |
| Phase 1 — Minimal runtime (state machine, JSONL replay, 1 e2e test) | ✅ Done |
| Phase 2.1 — Plan parser (Markdown → DAG, strict mode) | ✅ Done |
| Phase 2.2 — Plan validator (cycles, budget, max-nodes) | ✅ Done |
| Phase 2.3 — Plan scheduler (topological, fair, repair subgraph) | ✅ Done |
| Phase 2.4 — React Flow UI for plan graph | 🛠 In progress |
| Phase 2.5+ — Memory + cost dashboard + marketplace | ⏳ Planned |
| v1.0 — Complete AI Software Company | 🎯 Target |

**Verified working end-to-end (v0.2.2):**
- 153 runtime tests passing
- Workflow `POST /api/workflow` → plan → workers → FinalReviewer → DONE
- `GET /api/workflow/{id}/plan` returns live `parsed_plan` + `task_statuses`
- OS-keychain secret store (Windows Credential Manager / macOS Keychain)
- Structured Plugin system (`echo`, `python`, `git`) with sandbox + default-deny write
- Per-agent timeouts (`*_timeout_seconds`) prevent local-model self-lock
- Windows installer: NSIS 3.6 MB + MSI 5.0 MB at [Release v0.2.2](https://github.com/Thatgfsj/AgentCompanyOS/releases/tag/v0.2.2)

See [plans/](./plans/) and [docs/ROADMAP.md](./docs/ROADMAP.md) for details.

---

## 🛠️ Tech Stack

Locked-in for v0.1 — see [docs/TECH_STACK.md](./docs/TECH_STACK.md) for the full picture.

* **Desktop shell:** Tauri v2 + Rust + React 19 + TypeScript + Vite
* **Frontend:** Tailwind v4 · shadcn/ui · Zustand · TanStack Query · Motion · Monaco · Xterm.js · React Flow
* **Backend (Rust):** Tokio · Tauri IPC · Serde · SQLx · SQLite + FTS5 · portable-pty · Crossbeam
* **AI Runtime (Python 3.12+):** FastAPI · Uvicorn · Pydantic v2 · asyncio · Loguru · Tenacity · Rich
* **Agent Framework:** Custom workflow engine, event bus, task-graph scheduler, prompt engine, model router
* **AI SDKs:** Anthropic · OpenAI · Google GenAI · MiniMax · Moonshot (Kimi) · DeepSeek · OpenRouter · Ollama · LM Studio · OpenAI-compatible (only `minimax` + `deepseek` verified end-to-end in v0.2.2; others implemented but not smoke-tested)
* **Execution:** Plugin ABC with structured args; `portable-pty` for Claude Code CLI as one possible backend, but **not** the only path — `echo`/`python`/`git` plugins ship in-tree
* **Plugins:** Structured plugin API (Python / git / echo shipped; MCP / Docker / Browser are stubs)
* **Testing:** Vitest · Playwright · cargo test · pytest
* **CI:** GitHub Actions (clippy · rustfmt · ruff · black · mypy · eslint · prettier)

---

## 🗂️ Repository Layout

```
AgentCompanyOS/
├── README.md              ← you are here
├── LICENSE                ← MIT
├── CONTRIBUTING.md        ← how to contribute
│
├── docs/                  ← 15 RFCs (PROPOSALS/, WORKFLOW_SPEC, TASK_GRAPH, …)
├── prompts/               ← 8 runnable agent prompts (Chief, Planner, …)
├── plans/                 ← 7 phase plans
├── .agent/                ← 4 rule files (CLAUDE.md etc.)
│
├── apps/                   ← runtime + desktop shells
│   ├── desktop/           ← Tauri v2 app (TypeScript + Rust)
│   └── runtime/           ← Python FastAPI sidecar (aco_runtime/)
│
├── packages/              ← pnpm workspace (TS shared types/components)
│   ├── ui/                ← React 19 components
│   ├── workflow/          ← workflow client types
│   ├── providers/         ← provider metadata
│   ├── prompts/           ← prompt renderer
│   └── shared/            ← cross-language types (WfEvent mirror)
│
├── crates/                ← Cargo workspace — Tauri-side glue only
│   ├── tauri-core/        ← Tauri commands, AppState
│   ├── event-bus/         ← Rust event types (mirror of shared/events.ts)
│   ├── claude-adapter/    ← portable-pty Claude Code CLI runner
│   ├── config/            ← aco.toml loader
│   └── storage/           ← sqlx + SQLite + FTS5 (Phase 2 stub)
│
├── runtime/               ← uv workspace — the actual product
│   ├── pyproject.toml
│   ├── src/
│   │   └── aco_runtime_lib/      ← the runtime library
│   │       ├── agents/            ← Chief, Planner, Worker, Critic, Reporter, FinalReviewer
│   │       ├── workflow/          ← orchestrator + state machine
│   │       │                       + plan_parser / plan_validator / plan_scheduler
│   │       ├── providers/         ← model router (12+ providers, keychain-backed)
│   │       ├── plugins/           ← Plugin ABC + builtin/{echo,python,git}
│   │       ├── prompts/           ← Jinja-style prompt renderer
│   │       ├── memory/            ← in-mem key-value (Phase 2 → SQLite)
│   │       ├── api/               ← routes (workflow, events, providers, settings, plugins)
│   │       ├── event_bus.py       ← pub/sub asyncio
│   │       ├── secrets.py         ← OS-keychain SecretStore
│   │       └── __init__.py
│   └── tests/              ← 153 pytest tests
│
├── .validation/           ← smoke + e2e + icon-gen scripts
├── .github/workflows/     ← CI (clippy · ruff · mypy · eslint · prettier)
└── target/release/        ← Tauri build artifacts (.exe, .msi, NSIS setup)
```

**Key paths:**
- Tauri desktop app: `apps/desktop/`
- Python AI runtime: `runtime/src/aco_runtime_lib/`
- Phase 2 plan parser / validator / scheduler: `runtime/src/aco_runtime_lib/workflow/`
- Plugin registry + builtin plugins: `runtime/src/aco_runtime_lib/plugins/`
- End-to-end demo: `python -m aco_runtime_lib.demo` (see below)

---

## 🚀 Quickstart (planned for Phase 0)

```bash
git clone https://github.com/Thatgfsj/AgentCompanyOS.git
cd AgentCompanyOS
pnpm install        # TypeScript deps
cargo build         # Rust deps
uv sync             # Python deps
make dev            # tauri dev + python sidecar
```

See [CONTRIBUTING.md](./CONTRIBUTING.md) for the full setup.

---

## 🤝 Contributing

This project follows **RFC-driven development**:

1. Open an issue describing the change.
2. If the change is non-trivial, draft a new RFC under `docs/`.
3. Discuss → revise → accept → implement.

See [CONTRIBUTING.md](./CONTRIBUTING.md) for the full contribution process.

---

## 📄 License

[MIT](./LICENSE) — see [LICENSE](./LICENSE) for the full text.

---

**Author:** Thatgfsj
**Created:** 2026-06-18
**Repo:** https://github.com/Thatgfsj/AgentCompanyOS
