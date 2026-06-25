# Flowntier

> **A Visual AI Software Company Powered by Multi-Agent Workflow**
>
> **Status:** v0.4 in development — single-process Rust runtime
> (no Python sidecar). Acceptance run #3 reported **PASS** with
> 28 backend tests + 6 Playwright scenarios on 2026-06-24.
>
> **Latest release:** v0.2.5 (Tauri installer build verified)
>
> **Author:** Thatgfsj
> **License:** MIT

Flowntier is not another AI IDE. It is an **AI Software Company
Operating System**.

Users interact with a visual workspace while multiple AI agents
collaborate behind the scenes — exactly like a real software
company:

```
需求  →  主理 (Chief)  →  计划 (Planner)  →  实施 (Worker) × N
                                              ↓
                 评审  ←  审查 (Reviewer) / 找茬 (BugHunter)
                                              ↓
                 交付  ←  汇报 (Reporter)  →  需求方
```

The IDE is only the visualization layer. **The workflow is the
product.**

---

## ✨ What's new in v0.4

* **All-Rust runtime** — the Python FastAPI sidecar and the
  Claude Code CLI wrapper (`crates/claude-adapter/`) are
  deleted. `crates/agent-core` is a single-process implementation
  of the agent loop, tool registry, and provider clients.
  See `docs/ACCEPTANCE_v0.4.md` for the boundary-test report
  that surfaced 8 latent bugs in the previous acceptance runs.
* **Capabilities** — `ToolContext` exposes per-tool `read /
  write / bash / network` flags, plus `read_only()`,
  `no_modify()`, and `network_off()` presets. The agent loop
  honors `CancellationToken` and tree-kills the entire
  child process tree on cancel.
* **Repeat-failure abort** — if the same `(tool, args)` pair
  fails three times in a row the loop emits
  `Done { status: "ABORTED_REPEAT" }` instead of burning the
  whole iteration budget on a loop.
* **Role prompts rewritten** — every role now has a uniform
  prompt skeleton (Identity / Responsibility / Out-of-scope /
  Workflow / Output format / Tools). The Worker prompt
  explicitly warns about the "defined but not wired up"
  anti-pattern that hit the v0.3 ledger acceptance.
* **Provider URL validation** — `validate_base_url()` rejects
  bad URLs (`https:/x.test`, `ftp://`, …) before they waste a
  TLS handshake.
* **Strict acceptance test harness** — 28 backend cases + 6
  Playwright scenarios. Catches FK violations, wrong response
  shapes, missing CORS preflight, and concurrency failures
  that previous runs missed.

---

## 🚀 Install (Windows)

Download the installer for the latest release from
[Releases](https://github.com/Thatgfsj/Flowntier/releases).

| Artifact | When to use | Size |
|----------|-------------|------|
| `Flowntier_0.2.5_x64-setup.exe` (NSIS) | Public distribution | ~30 MB |
| `Flowntier_0.2.5_x64_en-US.msi` (MSI) | Corporate / Group Policy | ~40 MB |
| `flowntier-desktop.exe` (standalone) | Dev / portable | ~16 MB |

SmartScreen shows "Unknown publisher" because the build is
unsigned. Click **More info → Run anyway**. Code-signing
documentation: [`docs/INSTALLER.md`](./docs/INSTALLER.md).

**No Python, no Node.js, no toolchain on the user's machine.**
The installer bundles everything needed.

---

## 🛠️ Build from source

Prerequisites: Rust ≥ 1.85, Node ≥ 24, pnpm ≥ 9, Tauri CLI,
WiX (Windows only, automatic).

```bash
git clone https://github.com/Thatgfsj/Flowntier.git
cd Flowntier

pnpm install

# Build the Rust sidecar first (flowntier-runtime + agent-core + pipe-server).
cargo build --release -p pipe-server

# Stage the sidecar binary where the Tauri bundler expects it.
mkdir -p apps/desktop/src-tauri/binaries
cp target/release/flowntier-runtime.exe \
   apps/desktop/src-tauri/binaries/flowntier_runtime-x86_64-pc-windows-msvc.exe

# Build the installer.
cd apps/desktop
pnpm tauri:build
```

The .msi and NSIS installers land in
`apps/desktop/target/release/bundle/`. ~8 minutes from clean
build, ~30 s incremental.

To run the React UI without packaging:

```bash
cd apps/desktop
pnpm tauri:dev
```

---

## 📚 Architecture in one minute

```
┌──────────────────────────────────────────────────────────────────┐
│                  Tauri Webview (React 19 + Vite)                 │
│  ChatZone  MissionControl  Settings  BottomConsole  CommandDock  │
└─────────────────────────┬────────────────────────────────────────┘
                          │ tauri::command
┌─────────────────────────┴────────────────────────────────────────┐
│                flowntier-runtime (Rust, single process)                │
│                                                                  │
│   crates/pipe-server    JSON-RPC over \\.\pipe\flowntier_runtime       │
│     ↓                                                            │
│   crates/agent-core     in-process agent loop + tool registry    │
│     ↓                  + provider clients (SSE)                   │
│     ↓                                                            │
│   crates/event-bus      pub/sub (Rust ↔ Tauri webview)           │
│     ↓                                                            │
│   crates/storage        sqlx + SQLite (workflows, usage)          │
│                                                                  │
│   crates/provider-presets  built-in OpenAI/Anthropic/...         │
└──────────────────────────────────────────────────────────────────┘
                          │ HTTPS
                          ▼
                  LLM Provider APIs
```

See [`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md) for the full
module-boundary spec and data-flow diagrams.

---

## 🗂️ Repository layout

```
Flowntier/
├── apps/
│   └── desktop/                # Tauri v2 app (React + Rust sidecar)
│
├── crates/                     # Cargo workspace — all Rust
│   ├── agent-core/             # ⭐ in-process agent loop, tools, providers
│   ├── pipe-server/            # ⭐ JSON-RPC + event-push over named pipe
│   ├── event-bus/              # Rust pub/sub
│   ├── tauri-core/             # Tauri app glue
│   ├── config/                 # flowntier.toml loader
│   └── storage/                # SQLx repositories
│
├── packages/                   # pnpm workspace (TS shared)
│   ├── ui/                     # React 19 components
│   ├── workflow/               # workflow client types
│   ├── providers/              # provider metadata tables
│   ├── prompts/                # prompt renderer (TS)
│   └── shared/                 # cross-language types
│
├── docs/                       # RFCs and acceptance reports
│   ├── ROADMAP.md              # milestones, versioning
│   ├── ARCHITECTURE.md         # data flow, module boundaries
│   ├── TECH_STACK.md           # locked-in tech
│   ├── AGENT_PROTOCOL.md       # inter-agent envelopes
│   ├── DEPENDENCIES.md         # cargo-deny advisory summary
│   ├── INSTALLER.md            # build steps, signing, auto-update
│   ├── ACCEPTANCE_v0.3.md      # first acceptance run
│   ├── ACCEPTANCE_v0.3_LEDGER.md
│   ├── ACCEPTANCE_v0.4.md      # ⭐ strict acceptance (28 backend + 6 UI)
│   ├── DEPRECATIONS.md
│   └── ...
│
├── .agent/                     # repository-wide rules
├── .github/workflows/          # CI
├── acceptance/                 # one-shot acceptance artefacts
└── Cargo.lock                  # committed (apps are binaries)
```

> **v0.3 → v0.4 migration:** the `runtime/` Python package,
> `apps/runtime/` Python sidecar, and `crates/claude-adapter/`
> are gone. There is no Python on the runtime path. See
> [`history/docs/V03_DELETIONS.md`](./history/docs/V03_DELETIONS.md) for the removal record.

---

## 🔬 Test the runtime end-to-end

```bash
# Run all Rust tests (54 unit + 6 e2e).
cargo test --workspace

# Run the strict acceptance (separate Python harness):
#   1. Start backend
mkdir -p acceptance/ledger-task/backend
cd acceptance/ledger-task/backend
node server.js &
#   2. Start frontend
cd ../frontend
python -m http.server 5501 &
#   3. Run the harness
python ../../backend_strict_test.py    # 28/28
python ../../frontend_visual_test.py   # 6/6 scenarios with screenshots
```

Both harness scripts are committed alongside this README in the
working acceptance directory. See [`docs/ACCEPTANCE_v0.4.md`](./docs/ACCEPTANCE_v0.4.md)
for what they actually catch.

---

## 🧠 Roles

| 中文 | English | 职责 |
|---------|---------|------|
| 主理 | Chief | 接收需求、调度团队、最终交付 |
| 计划 | Planner | 产出 Markdown 方案,不写代码 |
| 实施 | Worker | 写代码、改文件、跑命令 |
| 找茬 | BugHunter | 只读;找 Bug、安全漏洞、边界情况 |
| 审查 | Reviewer | 几乎只读;检查命名、抽象、测试 |
| 汇报 | Reporter | 用大白话给需求方做最终总结 |

System prompts live in `crates/agent-core/src/prompt/mod.rs`.

---

## 📊 Roadmap

| Milestone | Status |
|-----------|--------|
| v0.1 — Foundation (monorepo, CI, RFCs, Windows installer) | ✅ shipped |
| v0.2 — Multi-provider maturity | ✅ shipped |
| v0.3 — Embedded Rust agent + Chat Zone | ✅ shipped |
| **v0.4 — Capabilities + repeat-failure abort + strict acceptance** | ✅ **shipped** |
| v0.5 — Bash output streaming + multi-wf isolation + tree-kill | ⏳ in progress |
| v0.6 — Memory + replay | 🎯 next |
| v1.0 — Production | 🎯 target |

The full milestone list with exit criteria is in
[`docs/ROADMAP.md`](./docs/ROADMAP.md).

---

## 🤝 Contributing

RFC-driven development. Non-trivial changes start as a
document under `docs/`, get reviewed, then implemented.

Read [`CONTRIBUTING.md`](./CONTRIBUTING.md) and the
repository-wide rules in [`.agent/`](./.agent/) before opening a
PR. The CI is strict: clippy pedantic, rustfmt, eslint,
prettier, and 60+ unit tests must all pass.

---

## 📄 License

[MIT](./LICENSE) — see [LICENSE](./LICENSE) for the full text.

---

**Author:** Thatgfsj
**Repository:** https://github.com/Thatgfsj/Flowntier
