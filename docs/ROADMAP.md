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
LLM streaming all live inside the Tauri process. The UI gets a
**Chat Zone** docked next to the existing Settings — progressive,
not a full IDE rewrite.

**LLM provider strategy:** One wire format (OpenAI Chat Completions +
SSE). Anthropic is reached either through OpenAI-compat proxies
(AWS Bedrock, LiteLLM, etc.) or via a thin adapter that translates
Anthropic Messages API ↔ OpenAI on the fly. No second first-class
provider client.

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

### v0.3 — Embedded Agent (Rust) + Chat Zone ✅ **shipped 2026-06-22**

> **Title:** Kill the sidecar. Add a chat box.

**Done criteria:**

- [x] **`crates/agent-core`** — in-process agent loop, tool trait,
  context window manager, SSE streaming *(shipped W1)*
- [x] **Provider: OpenAI-compat only** — Anthropic supported through
  an OpenAI-compat adapter layer (vendor proxies); no separate
  Anthropic-native client. Verified live against MiniMax-M3.
- [x] **Tool system** — `read` / `write` / `patch` / `bash` / `grep`
  all in Rust *(shipped W1)*
- [x] **`crates/pipe-server`** — Rust named-pipe server replaces the
  Python FastAPI runtime *(shipped W3)*. Verified end-to-end
  with external PowerShell named-pipe client.
- [x] **Python runtime deleted** — `apps/runtime/`, `runtime/`,
  and `crates/claude-adapter/` removed; desktop builds and
  runs without Python on PATH. See `docs/V03_DELETIONS.md`
  for the full record.
- [x] **Chat Zone (progressive)** — new chat zone docked next to
  Settings. NOT a full IDE rewrite — same 5-zone layout, just
  with a chat panel for sending tasks to the agent. Streaming
  text deltas + tool timeline stream live through
  `useAgentStream`. *(shipped W4)*
- [x] **Streaming** — text deltas, tool started/finished, file diffs
  all arrive live; user sees activity in < 200 ms after prompt
  *(verified end-to-end with MiniMax-M3, see
  `docs/ACCEPTANCE_v0.3.md` §7)*
- [x] **Roles relabeled to 中文** — 首席 / 缺陷猎手 / 工匠 / 质检师
  / 军师 / 传令官 *(implemented in `crates/agent-core/src/prompt/` + UI strings in `ChatZone.tsx`)*
- [x] **Provider presets trimmed** — drop MiniMax/DeepSeek from
  built-in presets; MiniMax-Text-01 specifically (it does not
  support tool_calls). Users can still add them as custom relays
  via `Capabilities::default()` providers.
- [x] **No silent CLI spawns** — zero `Command::spawn` calls except
  inside the `bash` tool (which is user-initiated and visible).
  `cargo build` confirms 0 in production code.

**Anti-features (still must NOT exist):**

- ❌ WASM plugins (deferred)
- ❌ Multi-workflow parallelism
- ❌ Cloud sync
- ❌ Mobile / responsive layout < 768 px
- ❌ Voice / Live2D (deferred to v0.6)
- ❌ Any Python in the runtime path

---

### v0.4 — Safety Hardening ✅ shipped 2026-06-22

> **Title:** Stop the agent from doing dumb things in a tight loop.

**Done criteria:**

- [x] **`Capabilities` per `ToolContext`** — `read` / `write` /
  `bash` / `network`. Every built-in tool gates its `execute()`
  on the corresponding flag. Convenience constructors:
  `default()`, `read_only()`, `no_modify()`, `network_off()`.
  Real failures observed during v0.3 acceptance (model calling
  `npm install` against a Windows box with no BuildTools)
  motivated this; the model can now be told "no network" and
  it pivots.
- [x] **`looks_like_network(cmd)` heuristic** — conservative
  substring check that gates the `network` capability inside
  the `bash` tool. False positives (refused when allowed) are
  cheaper than false negatives.
- [x] **`AgentConfig::repeat_abort_after`** — default 3. When
  the same `(tool_name, normalised_args)` pair fails this
  many times in a row, the loop emits
  `Done { status: "ABORTED_REPEAT" }` and exits, saving
  provider round-trips. `stable_hash` makes JSON key order
  irrelevant.
- [x] **`Capabilities` added to `Done` status**
  (`pipe-server/src/handlers.rs`) — `ABORTED_REPEAT` is
  treated as a terminal status alongside `DONE` /
  `FAILED` / `ABORTED`.

**Verification (acceptance run, 37 tests total):**

- `repeat_failure_aborts_before_max_iterations` (e2e) —
  scripts 3 identical bash refusals, asserts exactly 3
  failures + `ABORTED_REPEAT` + no provider call beyond.
- `read_only_capability_blocks_write_tool` (e2e) —
  verifies the `write` tool refuses with
  `Capabilities::read_only()`.
- 9 new unit tests covering capability presets, the network
  heuristic (positive + negative cases), `stable_hash`
  key-order independence, and bash gating.

**Anti-features (still must NOT exist):**

- ❌ Per-tool capability negotiation with the LLM (no
  "please approve this dangerous op" prompt yet; v0.5)
- ❌ Workspace capability token (no `path_allowlist`; the
  whole workspace is trusted. v0.5.)
- ❌ Resource quotas (max files written, max bytes served
  by `read`, etc.)

**API changes from v0.3 (worth noting if/when we publish):**

- `agent_core::message::ChatMessage` was a `type ChatMessage
  = Message;` alias. Removed because nobody used it; the
  one true name is `agent_core::Message`.
- `agent_core::message::ToolResult` was a struct that was
  never constructed; loop_.rs always built a `Message::tool`
  directly. Removed.
- `pipe_server::{Handler, HandlerFuture}` were types
  internal to `Dispatcher`; removed from the public
  re-export list.
- `agent_core::tool::Capabilities` is now publicly
  re-exported under `agent_core::Capabilities`.

No external users yet (v0.x pre-release); if/when this becomes
a published crate, these removals would warrant a `v0.4.0`
minor bump under semver, or a `v0.3.1` with a `#[deprecated]`
shim if backwards compatibility matters.

---

### v0.5 — Memory & Replay

> **Title:** ACO remembers; you can rewind.

- [ ] Project memory (persistent facts across workflows)
- [ ] Workflow replay (JSONL + model versions → bit-exact rerun)
- [ ] Plan doc rendered as an editable graph
- [ ] Multi-workflow parallelism with separate 首席 instances
- [ ] Provider reliability learning (router downranks bad providers)
- [ ] WASM plugins
- [ ] i18n (UI + prompts fully bilingual)

### v0.6 — Personality

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