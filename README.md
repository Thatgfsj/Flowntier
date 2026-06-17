# Agent Company OS (ACO)

> A Visual AI Software Company Powered by Multi-Agent Workflow

[![Status](https://img.shields.io/badge/status-v0.1%20RFC%20Draft-blue)]()
[![License](https://img.shields.io/badge/license-TBD-lightgrey)]()

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

| Document | Purpose |
|----------|---------|
| [PROJECT_SPEC.md](./PROJECT_SPEC.md) | Top-level product vision, philosophy, agents, workflow, roadmap |
| [docs/UI_GUIDELINES.md](./docs/UI_GUIDELINES.md) | Mission Control UI design system, layout, components, themes |
| [docs/AGENT_PROTOCOL.md](./docs/AGENT_PROTOCOL.md) | Inter-agent contract, message format, task lifecycle |
| [docs/PROVIDER_SPEC.md](./docs/PROVIDER_SPEC.md) | Multi-provider model layer (Anthropic / OpenAI / Gemini / etc.) |
| [docs/WORKFLOW_SPEC.md](./docs/WORKFLOW_SPEC.md) | State machine for the 8-phase workflow |
| [docs/PROMPT_GUIDE.md](./docs/PROMPT_GUIDE.md) | Per-agent prompt templates and authoring rules |
| [docs/PLUGIN_SPEC.md](./docs/PLUGIN_SPEC.md) | Plugin interface (Git / Docker / MCP / Browser / etc.) |
| [docs/ROADMAP.md](./docs/ROADMAP.md) | Long-term versioning, milestones, deprecations |

> **Convention:** Anything in `docs/` is a *Request For Comments* — proposals
> are reviewed before implementation. Once accepted, the RFC is the source of
> truth for that subsystem.

---

## 🏗️ Status

**Current version:** `v0.1` (RFC Draft)

| Milestone | Status |
|-----------|--------|
| v0.1 — Basic workflow + Chief + Critics + Workers + Claude Code + Simple UI | 📝 RFC Draft |
| v0.2 — Model routing, provider management, task history | ⏳ Planned |
| v0.3 — Project memory, workflow replay, planning visualization | ⏳ Planned |
| v0.4 — Plugin system, Git/Docker/MCP integration | ⏳ Planned |
| v0.5 — Live2D, animated agents, voice | ⏳ Planned |
| v1.0 — Complete AI Software Company | 🎯 Target |

See [docs/ROADMAP.md](./docs/ROADMAP.md) for details.

---

## 🛠️ Tech Stack

> **TBD** — to be decided after v0.1 RFCs are merged.

Candidate stack (under discussion):

* **Core runtime:** Rust _(per user preference for large systems)_
* **UI shell:** Tauri + React/TypeScript _(cross-platform desktop, Mission Control layout)_
* **Agent orchestration:** Rust core + Python ML glue _(via PyO3)_
* **Provider layer:** Multi-SDK abstraction (see [docs/PROVIDER_SPEC.md](./docs/PROVIDER_SPEC.md))
* **Execution engine:** Claude Code CLI (v0.1), with adapters for Codex / Aider / Gemini CLI

---

## 🤝 Contributing

This project follows **RFC-driven development**:

1. Open an issue describing the change.
2. If the change is non-trivial, draft a new RFC under `docs/`.
3. Discuss → revise → accept → implement.

See [CONTRIBUTING.md](./CONTRIBUTING.md) _(TODO)_ for details.

---

## 📄 License

TBD — see [LICENSE](./LICENSE) _(TODO)_.

---

**Author:** Thatgfsj
**Created:** 2026-06-18
