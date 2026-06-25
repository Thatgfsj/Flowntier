# Flowntier

> **A Visual AI Software Company Powered by Multi-Agent Workflow**
>
> **Status:** v0.4.0 ready for users — Windows + Linux installers
> build end-to-end through GitHub Actions. The 7-PR delivery
> chain (Phase 0–5 + release polish) shipped 33 commits across
> the v0.3 → v0.4 transition.
>
> **Latest release:** v0.4.0-rc1 (draft, see the
> [v0.4-delivery plan](#-release-status))
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

## 🆕 What's new in v0.4.0

v0.4.0 is the first release **aimed at real users**. Everything
in v0.3 that was a stub or TODO has been replaced with a working
implementation, and the user-facing failures from v0.3 (silent
crashes, settings that don't persist, no installer) are fixed.

* **🛡 Persistent secrets.** API keys entered in Settings now
  actually persist across quit+relaunch. Stored in AES-256-GCM
  ciphertext with a 32-byte DEK in the OS keystore:
  Credential Manager (DPAPI) on Windows, Keychain on macOS,
  libsecret on Linux. Fallback to a passphrase-protected file
  for headless Linux. See `docs/SECURITY.md §"Cryptographic
  posture"`.
* **🪟 Working installer.** GitHub Actions builds NSIS + MSI
  installers for Windows and .deb + AppImage for Linux on every
  `v*` tag. macOS deferred to v0.5 per chairman directive.
  See `docs/INSTALLER.md`.
* **🔄 Auto-update.** The app checks GitHub Releases on every
  launch and prompts the user when a newer version is
  available. Updates are signed with an ed25519 keypair; the
  public key is compiled into the shell. Signature verification
  is mandatory.
* **🎯 9 built-in LLM providers.** OpenAI, Anthropic, Google
  AI, DeepSeek, Moonshot Kimi, Zhipu GLM, Xiaomi MiMo, and
  SiliconFlow — pre-configured with sensible defaults. Custom
  providers (relay stations, private gateways) supported via
  the Settings UI.
* **🚨 Graceful error handling.** Three layers:
    - React ErrorBoundary: any uncaught exception shows a
      "复制日志 / 重启应用 / 上报问题" screen with a copyable
      stack trace and pre-filled GitHub issue URL.
    - Startup error dialog: if the runtime fails to initialize
      (data dir unwritable, SQLite migration error), the Tauri
      shell shows a native error dialog with the log path
      instead of crashing silently.
    - Persistent log file: every Rust log line + every React
      error lands in `<data_dir>/logs/flowntier.log.YYYY-MM-DD`
      so the user can attach it to a bug report.
* **🛡 Strict Content Security Policy.** Scripts restricted
  to `'self'` (with Tauri-injected nonces); connect-src limited
  to the LLM provider base URLs + IPC; frames / objects / form
  actions blocked. See `docs/SECURITY.md §"[P2] Content
  Security Policy"`.
* **🌍 i18n scaffolding.** `react-i18next` + language toggle
  in the TopBar. v0.4 ships zh-CN (default) + en-US (covers
  all new strings; legacy UI is still zh-CN only and tracked
  for v0.5).
* **👋 First-run Welcome.** 3-step wizard: pick a provider,
  try the "implement POST /auth/login" sample, enter the
  workspace. Dismissing once sets `first_run=false`; never
  shown again.
* **🔄 Sidecar version handshake.** `GET /api/rpc/version`
  returns the sidecar's version + `min_compatible`. Shell
  compares; on drift (e.g. user updated shell but not the
  sidecar binary), a non-blocking warning appears in the
  dashboard.

### 📦 Release status

The v0.4.0 release chain shipped as 6 PRs (Phase 0 → 5):
  - [PR #3: Phase 0](https://github.com/Thatgfsj/Flowntier/pull/3) — brand rename, source-of-truth version, doc stubs
  - [PR #4: Phase 1](https://github.com/Thatgfsj/Flowntier/pull/4) — release CI + Windows/Linux installers + auto-update
  - [PR #6: Phase 3](https://github.com/Thatgfsj/Flowntier/pull/6) — persistent secrets + provider endpoints + router fix
  - [PR #7: Phase 4](https://github.com/Thatgfsj/Flowntier/pull/7) — Welcome screen + first-run flow
  - [PR #8: Phase 5](https://github.com/Thatgfsj/Flowntier/pull/8) — rpc.version handshake + drift banner
  - Phase 6 (this branch) — README/FAQ polish + v0.4.0 tag

The draft Release `v0.4.0-rc1` is published on every push to a
`v*` tag; the maintainer promotes it to public after smoke-testing
on a clean Windows VM and a clean Ubuntu 22.04 VM.

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
