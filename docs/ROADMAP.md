# Roadmap

> Long-term vision, milestones, and versioning policy for Agent Company OS

**Version:** v0.3 RFC
**Status:** Active
**Author:** Thatgfsj
**Last updated:** 2026-06-22

---

## 1. Vision (one paragraph)

ACO becomes the **operating system for an AI software company** — a
visual workspace where specialized AI agents collaborate under a 首席
to ship production software, while humans stay in control of intent and
final approval. The IDE is the surface; the workflow is the product.

Starting in **v0.3**, ACO is **all-Rust, embedded-agent**: no Python
sidecar, no external CLI to spawn. The agent loop, tool system, and
LLM streaming all live inside the Tauri process. The UI borrows
heavily from the Cursor / VSCode Chat experience: a single panel that
shows live file edits, terminal output, and timeline events as the
agent works.

---

## 2. Versioning Policy

ACO follows **strict semver**:

* **Major** (`v0.x` → `v1.0`): any breaking change to:
  * `agent-protocol/vX` schema
  * `provider-spec` Provider trait
  * `workflow-spec` state machine
  * `plugin-spec` manifest schema
* **Minor** (`v0.1` → `v0.2`): additive only. New agents, new providers,
  new capabilities, no removals.
* **Patch** (`v0.1.0` → `v0.1.1`): bug fixes, prompt tweaks, perf.

Pre-`v1.0`, **every minor is allowed to break** with a deprecation
note in the release log. Post-`v1.0`, no breaking changes inside a
major.

---

## 3. Roles (中文)

> v0.3 renames agent roles to Chinese with memorable, non-robotic labels.

| 旧名 | 新名 | 职责 |
|------|------|------|
| Chief | **首席** | 拆任务、调度、统筹 |
| Planner | **军师** | 出方案、对比、选型 |
| Reviewer | **质检师** | 看代码、挑刺、写 review |
| Critic | **缺陷猎手** | 挖 bug、安全漏洞、边界情况 |
| Worker | **工匠** | 写代码、修文件、跑命令 |
| Reporter | **传令官** | 给用户写最终摘要 |

---

## 4. Milestones

### v0.1 — Foundation ✅ shipped (2026-05)

CLI-driven agents, in-process workflow, JSONL persistence. Basic UI
with 5 zones.

### v0.2 — Multi-Provider Maturity ✅ shipped (2026-06)

All major providers wired, custom relay support, cost tracking,
plugin UI panels.

### v0.3 — Embedded Agent (Rust) + IDE UI ⏳ **current**

> **Title:** Kill the sidecar. Live like an IDE.

**Done criteria:**

- [ ] **`crates/agent-core`** — in-process agent loop, tool trait,
  context window manager, SSE streaming
- [ ] **Provider trait (Rust)** — OpenAI, Anthropic, Gemini all
  implemented natively with `reqwest` + SSE parsing
- [ ] **Tool system** — `read` / `write` / `patch` (unified-diff apply)
  / `bash` (tokio::process) / `grep` / `glob` all in Rust
- [ ] **Patch algorithm** — bidirectional fuzzy match (not a CLI
  dependency); `.bak` backup before any write
- [ ] **`crates/pipe-server`** — Rust named-pipe server replaces the
  Python FastAPI runtime. JSON-RPC protocol over
  `\\.\pipe\aco_runtime` + `\\.\pipe\aco_runtime_events`
- [ ] **Python runtime deleted** — `apps/runtime/` and `runtime/`
  removed; `apps/desktop` builds and runs without Python on PATH
- [ ] **IDE-style single-page UI** — 4 zones:
  - left: live file tree (auto-refreshes after tool writes)
  - center: Monaco editor with diff view for active file
  - right: Timeline (agent actions streaming)
  - bottom: chat input + xterm.js console
- [ ] **Streaming** — text deltas, tool started/finished, file diffs
  all arrive live; user sees activity in < 200 ms after prompt
- [ ] **Roles relabeled to 中文** — 首席 / 缺陷猎手 / 工匠 / 质检师 / 军师
- [ ] **Provider presets trimmed** — drop MiniMax/DeepSeek from
  built-in presets (users can still add them as custom relays)
- [ ] **No silent CLI spawns** — zero `Command::spawn` calls except
  inside the `bash` tool (which is user-initiated and visible)

**Anti-features (still must NOT exist):**

- ❌ WASM plugins (deferred)
- ❌ Multi-workflow parallelism
- ❌ Cloud sync
- ❌ Mobile / responsive layout < 768 px
- ❌ Voice / Live2D (deferred to v0.5)
- ❌ Any Python in the runtime path

---

### v0.4 — Memory & Replay

> **Title:** ACO remembers; you can rewind.

- [ ] Project memory (persistent facts across workflows)
- [ ] Workflow replay (JSONL + model versions → bit-exact rerun)
- [ ] Plan doc rendered as an editable graph
- [ ] Multi-workflow parallelism with separate 首席 instances
- [ ] Provider reliability learning (router downranks bad providers)
- [ ] WASM plugins
- [ ] i18n (UI + prompts fully bilingual)

### v0.5 — Personality

> **Title:** ACO has a face.

- [ ] Live2D avatars for 首席 / 缺陷猎手 / 工匠 / 质检师
- [ ] Streaming "thinking" bullets visible mid-response
- [ ] Voice input (Whisper) and output (TTS) — user-optional

### v1.0 — Production

> **Title:** The complete AI Software Company.

- [ ] All v0.x features stable
- [ ] 100-task workflow completes on a laptop in < 30 min wallclock
- [ ] Observability: structured logs, Prometheus metrics, OTel tracing
- [ ] 3rd-party security audit
- [ ] `agent-protocol/v1` schema frozen

---

## 5. Non-Goals (permanent)

* ❌ A general-purpose LLM chat UI
* ❌ A code editor (ACO is a *workspace*, not an editor — it borrows
  Monaco for *viewing* diffs only)
* ❌ A version control system
* ❌ A CI/CD platform
* ❌ Multi-tenant / enterprise SSO
* ❌ Replacing the user's judgment

---

## 6. Success Metrics

| Metric                                              | Target (v1.0) |
|-----------------------------------------------------|----------------|
| Time from "I have an idea" to "running code"        | < 2 min       |
| Latency: prompt → first text delta on screen        | < 200 ms      |
| Number of human interventions per 10-task workflow  | < 3            |
| % of workflows that pass 缺陷猎手 review on 1st try | > 70%         |
| Token cost per 10-task workflow (median)            | < $5 USD       |
| Workflows replayable bit-exact                      | 100%          |

---

## 7. Release Cadence

* **v0.2 → v0.3:** target 6 weeks after v0.3 starts (current)
* **v0.3 → v0.4:** target 12 weeks
* **v0.4 → v0.5:** target 12 weeks
* **v0.5 → v1.0:** target 16–24 weeks (longer because of audit + freeze)

---

## 8. Open Questions

1. Should v0.4 ship a **plugin marketplace** before WASM plugins
   (no third-party code without sandboxing)?
2. Should v0.5's Live2D avatars be **open-source assets** the user
   can swap, or first-party only? (proposed: open-source, swappable)
3. Should we ship **cloud sync** opt-in as a paid feature before v1.0
   to fund development? (proposed: no, keep v0.x local-only)

---

**RFC ends.**