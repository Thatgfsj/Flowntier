# Flowntier v0.3.0

The product is renamed from **Agent Company OS** (ACO) to **Flowntier** ("Flow" + "Frontier"). Same code, same architecture, new name. The Rust runtime, the Tauri desktop shell, and the workflow engine are otherwise unchanged from v0.2.5.

This release exists solely to ship the rename atomically. The next release (v0.4) is where the in-flight strict-acceptance work (8 bugs from the v0.4 boundary tests) will be cut.

## Highlights

- **Brand: Agent Company OS → Flowntier.** App, installer, config, prompts, npm packages, and active docs all use the new name.
- **Binaries:** `aco` → `flowntier` (CLI); `aco-runtime` → `flowntier-runtime` (sidecar). Source filenames kept so `git log --follow` still works.
- **Tauri identifier:** `dev.acos.desktop` → `ai.flowntier.desktop`. New bundle id; new installer code-signing identity.
- **npm scope:** `@aco/*` → `@flowntier/*`. Every workspace import path updated; `pnpm install` regenerates `pnpm-lock.yaml`.
- **Runtime data migration:** the legacy `~/.config/aco/` and `~/.local/share/aco/` (or `%APPDATA%/aco/` on Windows) are auto-detected on first launch and renamed in place to `flowntier/`. Logs one line to stderr. Best-effort — if the rename fails (perms, locked file), the new path is used and the legacy dir is left alone.
- **DB schema:** new migration `0002_rename_aco_to_flowntier.sql` runs `ALTER TABLE config_snapshots RENAME COLUMN aco_toml TO flowntier_toml`. Existing v0.2.x DBs upgrade in place.
- **Windows named pipes:** `\\.\pipe\aco_runtime` → `\\.\pipe\flowntier_runtime`. Unix domain sockets move accordingly.

## Breaking

For **end users** (the visible one):

- The Tauri bundle identifier is `ai.flowntier.desktop`, no longer `dev.acos.desktop`. The Windows installer will not auto-upgrade over v0.2.5; you'll get a fresh install. The runtime data-dir migration picks up your existing state on first launch and moves it to the new location.
- macOS / Linux: not affected on this release (no v0.2.5 binaries for those platforms yet). When they ship, the `~/.config/aco/` and `~/.local/share/aco/` paths will be migrated the same way.

For **plugin / extension authors**:

- None. The plugin manifest schema, manifest signing, and runtime ABI are unchanged.

For **contributors**:

- npm imports changed from `@aco/*` to `@flowntier/*`. Run `pnpm install` after pulling.
- The Rust config crate's public type renamed: `AcoConfig` → `FlowntierConfig`; `load_aco_config()` → `load_flowntier_config()`. No external consumers yet (this is the first public surface), so internal call sites in `tauri-core` are the only thing updated.
- The CLI binary `flowntier` replaces `aco`; `flowntier-runtime` sidecar replaces `aco-runtime`. Update any local scripts or Makefile targets that referenced the old names.
- The `.agent/coding_rules.md` env-var prefix is now `FLOWNTIER_*` (was `ACO_*`). e.g. `FLOWNTIER_LOG_LEVEL`, `FLOWNTIER_DATA_DIR`. The Tauri shell does not yet honor these overrides (TODO; tracked separately from this rename).

## Architecture

Unchanged from v0.2.5. The runtime diagram in `README.md` still describes the same Rust sidecar + Tauri webview + IPC-over-named-pipe layout.

## Verified

- `cargo test --workspace` — 71 tests pass, 0 fail (`agent-core` 54, `pipe-server` 4, `e2e_pipe` 3, `e2e_agent_loop` 6, `storage` 1, `config` 2, `event-bus` 1, others 0).
- `pnpm install` — `pnpm-lock.yaml` regenerated; all `@flowntier/*` packages resolve.
- `pnpm --filter @flowntier/desktop typecheck` — 0 errors.
- `pnpm -r typecheck` for `packages/{prompts,providers,shared,ui}` — 0 errors.
- `packages/workflow` typecheck: pre-existing failure on `@tauri-apps/api/core` dynamic import (not declared as a dependency). Unrelated to the rename; left as-is.
- `cargo build` — `flowntier.exe` and `flowntier-runtime.exe` produced under `target/debug/`. Release-mode installer build (`pnpm tauri:build`) deferred to next release.
- `flowntier doctor` end-to-end smoke (see below).

## Smoke (PowerShell, `.validation/i_ching_e2e.ps1`)

- Spawns `flowntier-runtime.exe`, waits for the named pipe,
  sends the JSON-RPC request, prints the response. Verified
  working end-to-end (sample draws: #53 渐, #61 中孚, #58 兑).
- This is the same path the Tauri webview takes on click,
  just driven from PowerShell for headless verification.

Co-authored-by: ZCode (MiniMax-M3) <noreply@example.invalid>

## Files changed

68 files in the single rename commit (`feat!: rename Agent Company OS to Flowntier`). Highlights:

| File / area | Change |
|---|---|
| `apps/desktop/src-tauri/tauri.conf.json` | `productName` + `identifier` + `externalBin` (`binaries/flowntier_runtime`). |
| `apps/desktop/src-tauri/Cargo.toml` | `description`; bind `[[bin]] name = "flowntier"` for the CLI. |
| `apps/desktop/src-tauri/capabilities/default.json` | Capability `description`. |
| `apps/desktop/index.html`, `App.tsx`, `StartupScreen.tsx`, `lib/api.ts`, `hooks/*` | Visible brand strings + comment headers. |
| `apps/desktop/package.json` + `packages/*/package.json` | npm `name` field, dependency references. |
| `pnpm-lock.yaml` | Regenerated by `pnpm install`. |
| `crates/tauri-core/src/bin/aco.rs` | `clap name = "flowntier"`, CLI strings. Source file kept as `aco.rs` for `git log --follow`. |
| `crates/pipe-server/src/bin/aco-runtime.rs` | Log strings. Source file kept as `aco-runtime.rs`. |
| `crates/tauri-core/Cargo.toml`, `crates/pipe-server/Cargo.toml` | `[[bin]]` blocks binding artifact names. |
| `crates/config/src/lib.rs` | `AcoConfig` → `FlowntierConfig`; `load_aco_config()` → `load_flowntier_config()`; default path `~/.config/aco/` → `~/.config/flowntier/` + legacy-dir migration. |
| `crates/storage/src/lib.rs` | `default_data_dir()` joins `flowntier/`; legacy `aco/` migrated on first call. |
| `crates/pipe-server/src/server.rs` + `handlers.rs` + `tests/e2e_pipe.rs` | Pipe names, Unix socket names, runtime string, test assertions. |
| `crates/agent-core/src/prompt/mod.rs` | All 6 role prompts ("你是 Flowntier 的「...」"). |
| `crates/agent-core/examples/{acceptance_admin,orchestrated_planner,smoke_minimax_tools}.rs` | Temp-dir names. |
| `crates/storage/migrations/0002_rename_aco_to_flowntier.sql` | **New** — `RENAME COLUMN aco_toml TO flowntier_toml`. |
| `config/aco.toml` → `config/flowntier.toml` | Renamed via `git mv`. Header comment updated. |
| `README.md`, `docs/{ARCHITECTURE,INSTALLER,ROADMAP,TECH_STACK}.md` | Activity files; brand + path strings. |
| `.github/workflows/release.yml` | `releaseName` template. |
| `.agent/{architecture,coding,commit}_rules.md` | Brand strings; env-var prefix `ACO_*` → `FLOWNTIER_*`. |
| `Makefile` | Header + `--filter @flowntier/desktop` targets. |
| `.gitignore` | Comment header. |

Historical `.validation/release_notes*.md` and the pre-2026-06-22 docs/* are intentionally left untouched — they describe v0.2-era designs and would be misleading if rewritten.

## Installers

Not yet rebuilt at this commit. To produce v0.3.0 installers after pulling:

```bash
cargo build --release -p pipe-server
mkdir -p apps/desktop/src-tauri/binaries
cp target/release/flowntier-runtime.exe \
   apps/desktop/src-tauri/binaries/flowntier_runtime-x86_64-pc-windows-msvc.exe
cd apps/desktop && pnpm tauri:build
```

Expected output filenames (matching the renamed `productName`):

| Artifact | Path | Size (Windows, x64) |
|---|---|---|
| Standalone `.exe` | `target/release/flowntier-desktop.exe` | ~16 MB |
| MSI installer | `target/release/bundle/msi/Flowntier_0.3.0_x64_en-US.msi` | ~40 MB |
| NSIS setup | `target/release/bundle/nsis/Flowntier_0.3.0_x64-setup.exe` | ~30 MB |

## What's NOT in this release

- Strict acceptance fixes (8 bugs from the v0.4 boundary tests). Those land in v0.4.
- A real installer build (no `.msi` / NSIS binary is committed at this commit).
- A new GitHub repository URL. The repo stays at `https://github.com/Thatgfsj/Flowntier` until you decide whether to rename it on the GitHub side; that field references the actual path, not the brand.
- Updating the 6/18-era legacy docs (`FAQ.md`, `CONFIG.md`, `PROVIDER_SPEC.md`, `PLUGIN_SPEC.md`, `PROMPT_GUIDE.md`, `SECURITY.md`, `STORAGE_SPEC.md`, `UI_GUIDELINES.md`, `WORKFLOW_SPEC.md`, `AGENT_PROTOCOL.md`, `DEPENDENCIES.md`, `ISSUES_GRAPH.md`, `ACCEPTANCE_v0.3.md`, `ACCEPTANCE_v0.3_LEDGER.md`, `PROPOSALS/*`, `PROJECT_SPEC.md`, `CONTRIBUTING.md`, `plans/*`, `prompts/*`). These are intentionally preserved as historical snapshots.

## Full diff

Single commit: `feat!: rename Agent Company OS to Flowntier` (commit hash: see `git log --oneline`).