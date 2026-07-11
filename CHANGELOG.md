# Changelog

All notable changes to Flowntier are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

* `tools/replace_in_files.py` — UTF-8-safe text replace helper (replaces
  PowerShell `Set-Content -Replace` which mangles CJK characters on
  Windows).
* `Makefile` `lint-branding` target — CI guard that fails the build if
  the old `ACO` / `Agent Company OS` brand leaks back in outside of
  `history/`.

## [0.4.22] — 2026-07-02

Cumulative maintenance and bug-fix release that covers every
change made to the Flwntier master repo between v0.4.0 (the
"useable edition" milestone) and v0.4.22 (the current
release). The product line is **Flwntier desktop only** — the
Android "塔罗镜" v0.3.0 lives in a separate repo
(`Thatgfsj/tarot-oracle`) and has its own release line per
NWT 000077 / 000078.

### Added
* **Multi-agent orchestrator** (NWT 000068): 8-phase workflow
  (requirement → plan → plan-review → dispatch → develop →
  final-review → repair → delivery) implementing
  `history/PROJECT_SPEC.md` lines 60-280. The chief agent
  is the single source of truth; critic-a + critic-b run
  in parallel for plan and final reviews; workers run in
  parallel for development; each agent run writes one row
  to the `tasks` table so the dashboard's "任务列表"
  shows real per-agent progress (was a single-row stub
  before).
* **PlanDoc segmentation** (NWT 000069): the Plan phase now
  runs in 3 chief calls (Round A: summary + architecture,
  Round B: Backend/API/Database tasks, Round C:
  Frontend/Testing/Documentation tasks) with `merge()` for
  id-based dedup. Solves the 78-card-tarot timeout where
  the chief ran out of token budget streaming the full
  PlanDoc in one turn.
* **Async workflow kickoff + status poll** (NWT 000069):
  `POST /api/run_workflow` returns `wf_id` in ~50ms (was
  30+ min blocking); orchestrator runs in a `tokio::spawn`
  background task; clients poll `GET /api/workflow/{wf_id}/status`
  for the current phase + tasks_done/tasks_total counts.
* **PhaseTransition events** (NWT 000068) broadcast on the
  events pipe so the UI's PhaseTimeline animates as the
  workflow progresses.
* **Foreign-key safety** (NWT 000064): `ensure_workflow_row`
  in storage makes chat-derived `wf_chat_*` rows idempotent
  so the chief's `INSERT INTO tasks` doesn't get rolled back
  by the FK to `workflows.id`. Solves the chairman's
  "仪表盘永远 0/0 完成" complaint.
* **Dispatch /api/tasks?wf_id=…** endpoint (NWT 000064) so
  the dashboard can list per-agent rows under a given
  workflow.
* **App-side FileTree + ErrorBadge** (NWT 000066): the
  desktop app now has a left-side file tree (5s polling
  against the new `/api/workspace/tree` endpoint) and a
  TopBar red-dot badge that surfaces `GET /api/errors/recent`
  transient errors.
* **Workdir runtime swap** (NWT 000066): `Arc<RwLock<Workspace>>`
  on the runtime side + `POST /api/workspace/set` called by
  the Tauri shell's `set_workdir_with_nwt` so the runtime's
  chief filesystem context actually moves when the
  chairman changes the workdir in the desktop UI.
* **NSIS patch fix** (NWT 000066 / 000067): the previous
  `patch-nsis.cjs` taskkill belt-and-braces patch used
  `content.indexOf('{', idx)` which matched a brace in a
  header macro and inserted the block at the top of the
  file, breaking the .nsi syntax. Fixed by inserting
  immediately after `Function .onInit\n` instead of hunting
  for a brace that doesn't exist (NSIS functions are
  brace-less).
* **Apollo 8-stop** during the v0.4.0 → v0.4.22 work was
  needed because of the launch today, but no code was
  burned at the pad.

### Changed
* **tarot-oracle has been forked out** of the Flwntier
  master repo (NWT 000071 / 000073 / 000077 / 000078).
  The Android chief client code that lived at
  `apps/ChiefApp/` in v0.4.0 is gone; the same code is
  now at `Thatgfsj/tarot-oracle` under its own
  `v0.1.0` release. The Flwntier master only ships the
  desktop NSIS + the html-frontend cross-platform
  fast-path.
* **CHANGELOG and RELEASE notes brought up to date** with
  this entry. The previous `RELEASE_v0.4.md` is now
  `RELEASE_v0.4.22.md` (see the file rename in this
  release's commit log).
* **`docs/ACCEPTANCE_v0.3_stage_a.txt` /
  `docs/ACCEPTANCE_v0.3_stage_c.txt` removed** — those
  are the v0.3 acceptance runs from before the
  All-Rust rewrite. The current authoritative acceptance
  is `docs/ACCEPTANCE_v0.4.md`.

### Fixed
* `patch-nsis.cjs` (the previous broken taskkill insertion
  that put the block at the very top of the .nsi).
* `storage` foreign-key constraint blocking the chat
  workflow's `INSERT INTO tasks` (event 000064).
* `dispatcher` `?query` not matching (event 000064).
* `run_task` had no timeout (event 000064).
* `make_run_workflow` returned 30+ min for large requests
  (event 000064 → 000069 fix).
* Runtime `state.emit_phase` update was silently dropped
  because `update_workflow_state` was sync and emit_phase
  was sync (event 000069).
* `ensure_workflow_row` clobbered phase updates with
  'DONE'/'chat' (event 000069).
* `no_chairman-yet` no_chairman-yet (event 000068).

## [0.4.0] — 2026-06-25

The first release **aimed at real users**. v0.3 was a working v0.3; v0.4
closes every gap that prevented shipping to non-developers.

### Added (Phase 1)
* **`.github/workflows/release.yml`** — full release pipeline: Rust
  sidecar built in-job per target (`x86_64-pc-windows-msvc`,
  `x86_64-unknown-linux-gnu`); `tauri-action@v0` matrix; draft
  GitHub Release with NSIS + MSI + .deb + AppImage; updater
  signature verified with ed25519.
* **`.github/workflows/ci.yml`** — branding lint, cargo
  test+clippy+fmt, pnpm typecheck+eslint+prettier, e2e-windows,
  summary — replaces the broken v0.3 CI that referenced the deleted
  Python runtime.
* **`tauri-plugin-updater`** wired end-to-end: Rust plugin +
  capability + frontend `lib/updater.ts` + TopBar banner.
* **`apps/desktop/src-tauri/tauri.conf.json`** full bundle config
  (NSIS + WiX + Linux deb deps + macOS bundle config — kept so
  the chairman can re-enable macOS in v0.5 without code changes).
* **Tauri 2 icon set** — regenerated from `icon-256.png` (the old
  `icon-1024.png` was JPEG-disguised-as-PNG).

### Added (Phase 2)
* **`apps/desktop/src/components/ErrorBoundary.tsx`** — wraps the
  React tree; renders a "出错了 vX.Y.Z · Build <sha>" screen
  with "📋 复制日志 / 🔄 重启应用 / 🐛 上报问题" actions.
* **`react-i18next` + `i18next` bootstrap** — `zh-CN` default +
  `en-US` scaffold covering v0.4-introduced strings. `🌐 中文 / EN`
  toggle in TopBar.
* **`crates/tauri-core/src/logging.rs`** — daily rolling
  `<data_dir>/logs/flowntier.log.YYYY-MM-DD` + `std::panic::set_hook`
  writing panic info + `force_capture()` backtrace to
  `panic-<ts>.log`. `log_frontend_error` Tauri command wires the
  React side to the same file.
* **`tauri-plugin-dialog`** — graceful startup error dialog
  (native MessageBox / NSAlert / GTK MessageDialog) replacing
  silent `std::process::exit(1)`.
* **Strict CSP** in `tauri.conf.json`: `script-src 'self'` + Tauri
  nonces, `connect-src` whitelisted to LLM provider base URLs +
  IPC, `frame-src 'none'` / `object-src 'none'` / `form-action 'none'`.
* **`docs/SECURITY.md`** expanded with [P2] markers documenting the
  new threat model.

### Added (Phase 3 — the biggest single change in v0.4)
* **`crates/storage/migrations/0003_secrets_and_providers.sql`** —
  `secret` (encrypted), `provider` (presets + overrides),
  `custom_provider` (relay stations), `model_cache` (1h TTL),
  `kv` (flags). Pre-populates the 9 built-in providers.
* **`crates/pipe-server/src/secrets/`** — `SecretStore` backed by
  `keyring` crate (DPAPI / Keychain / libsecret) + AES-256-GCM
  with AAD binding + `FallbackKeychain` for headless Linux.
* **`crates/pipe-server/src/providers.rs`** — 9 built-in presets
  (OpenAI, Anthropic, Google, DeepSeek, Moonshot Kimi, Zhipu GLM,
  Xiaomi MiMo, SiliconFlow) + `ANTHROPIC_FALLBACK_MODELS` for
  Anthropic's missing `/v1/models` endpoint.
* **5 real handlers**:
  - `GET /api/settings/secrets` — metadata only, never ciphertext
  - `PUT /api/settings/secrets/{name}` — encrypt + store
  - `DELETE /api/settings/secrets/{name}`
  - `GET /api/settings/secrets/{name}/reveal` — internal plaintext
  - `POST /api/settings/secrets/seed` — v0.3 plaintext migration
* **6 real handlers** for providers:
  - `GET /api/providers` — joins presets + custom + `has_secret`
  - `PATCH /api/providers/{id}` — toggle / override
  - `GET /api/providers/{id}/models` — live /models fetch + cache
  - `POST /api/providers/custom`
  - `DELETE /api/providers/custom/{id}`
* **Dispatcher path-pattern matching** — `{name}` / `{id}`
  placeholders now extracted; v0.3 placeholder paths returned 404.
* **Fixed `/api/router/roles`** — was returning
  `anthropic:claude-sonnet-4` (a model id that doesn't exist).
  Now returns `claude-opus-4-8` with `claude-sonnet-4-6` fallback.

### Added (Phase 4)
* **`apps/desktop/src/components/Welcome.tsx`** — 3-step first-run
  flow (provider quick-add → sample workflow → enter workspace).
* **Tauri commands**: `kv_get`, `kv_set`, `first_run_complete`,
  `load_sample_workflow`.
* **App.tsx first-run gate** — reads `kv.first_run` on mount;
  renders Welcome or dashboard accordingly.

### Added (Phase 5)
* **`GET /api/rpc/version`** — returns sidecar version +
  `min_compatible`. App.tsx compares; if sidecar < min_compatible,
  renders a `DriftBanner` warning at the top of the dashboard.

### Added (Phase 6)
* **README + FAQ + CHANGELOG** polished for first user release.
* **macOS deferred to v0.5** per chairman directive.

### Removed
* **`apps/runtime/` + `crates/claude-adapter/`** — Python FastAPI
  sidecar + Claude Code CLI wrapper (Phase 0, commit `9527436`).
* **All `ACO_*` env vars** — renamed to `FLOWNTIER_*` (Phase 0).
* **`Agent Company OS` / `aco` / `@aco/*`** — full brand rename
  (Phase 0). Historical references live in `history/`.

### Verified
- 97 tests pass, 0 fail (`cargo test --workspace`)
- 8/8 e2e_pipe tests (5 new in v0.4)
- pnpm typecheck clean
- cargo build on Windows + Linux (per release.yml matrix)
- Phase 0 PR #3 / Phase 1 PR #4 / Phase 3 PR #6 / Phase 4 PR #7 /
  Phase 5 PR #8 all merged

### Known limitations (planned for v0.5)
* **No code signing** — SmartScreen shows "Unknown publisher"
* **macOS / iOS builds** — deferred per chairman directive
* **Full i18n coverage** — only zh-CN (default) + en-US (new strings
  only); legacy TopBar / Settings / CommandDock text still
  Chinese-only
* **CenterPanel not wired** — the component was refactored to
  support `hasActiveWorkflow=false` (empty-state card) but
  App.tsx still renders the demo content inline
* **Planned follow-ups**: macOS re-enable, code signing,
  full-i18n, CenterPanel wiring, advanced provider tests

### Changed

* **Brand rename complete.** All user-facing strings, file paths, crate
  descriptions, doc strings, package names, and binary names now use
  `Flowntier` consistently. The legacy `AcoConfig` Rust struct, the
  `aco_toml` SQL column, and the legacy `~/aco/` data-dir migration
  path are kept for compatibility and slated for removal in v1.0 —
  see `docs/DEPRECATIONS.md`.
* **Single source of truth for version.** `tauri.conf.json`, the
  desktop `Cargo.toml`, the workspace `Cargo.toml`, and
  `apps/desktop/package.json` now all read the same version
  (bumped together by `tools/bump_version.sh`, added in v0.4.1).
* **All-Rust runtime.** The Python FastAPI sidecar (`apps/runtime/`)
  and the Claude Code CLI wrapper (`crates/claude-adapter/`) are
  gone. `crates/agent-core` is a single-process implementation
  of the agent loop, tool registry, and provider clients.
  See `history/docs/V03_DELETIONS.md` for the full removal record.
* **Role prompts rewritten.** Every role now has a uniform prompt
  skeleton (Identity / Responsibility / Out-of-scope / Workflow /
  Output format / Tools). The Worker prompt explicitly warns about
  the "defined but not wired up" anti-pattern that hit the v0.3
  ledger acceptance.

### Added

* **Capabilities** — `ToolContext` exposes per-tool `read / write /
  bash / network` flags, plus `read_only()`, `no_modify()`, and
  `network_off()` presets.
* **CancellationToken honoured** — the bash tool kills the entire
  child process tree (via `taskkill /T` on Windows) when the user
  cancels a workflow.
* **Repeat-failure abort** — if the same `(tool, args)` pair fails
  three times in a row the loop emits
  `Done { status: "ABORTED_REPEAT" }` instead of burning the whole
  iteration budget on a loop.
* **Provider URL validation** — `validate_base_url()` rejects bad
  URLs (`https:/x.test`, `ftp://`, …) before they waste a TLS
  handshake.
* **Strict acceptance test harness** — 28 backend cases + 6
  Playwright scenarios. Catches FK violations, wrong response
  shapes, missing CORS preflight, and concurrency failures
  that previous runs missed. See `docs/ACCEPTANCE_v0.4.md`.

### Fixed

* **15 missing pipe-server handlers** — every endpoint the Tauri
  shell calls (`/api/settings/secrets/*`, `/api/router/roles`,
  `/api/plugins/*`, `/api/workflow/:id/cancel`, etc.) is now
  registered. Previously they returned 404 from the dispatcher.
* **Dispatcher keyed on `(method, path)`** instead of `path` alone,
  which had been silently overwriting `GET` and `PUT` handlers on
  the same path.
* **64-hexagram (I Ching) oracle** — rebuilt as a real module
  (`crates/pipe-server/src/i_ching.rs` + `hexagrams.json`) with
  5 unit tests; the Tauri shell exposes it via the `draw_i_ching`
  command and a Compose-styled React zone (`IChingOracle.tsx`).

## [0.3.0] — 2026-06-20

Last release before the All-Rust migration. Embedded Rust agent
loop lands. Windows NSIS + MSI installer built and verified.
See `history/release-notes/release_notes_v0.3.md`.

## [0.2.5] — 2026-06-15

Multi-provider maturity. 9 provider presets, real WebView2
bootstrapper, first GitHub Release with working installers.
See `history/release-notes/release_notes_v0.2.5.md`.

## [0.2.3] — 2026-06-08

Bug-fix release. See `history/release-notes/release_notes_v0.2.3.md`.

## [0.2.2] — 2026-06-01

Bug-fix release. See `history/release-notes/release_notes_v0.2.2.md`.

## [0.1] — 2026-05-15

Foundation. Cargo + pnpm monorepo, RFC-driven, first NSIS build.