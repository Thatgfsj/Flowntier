# Security

> Flowntier security posture. Status: **v0.4** (Phase 2 additions
> marked with `[P2]`).

**Last updated:** 2026-06-25
**Maintainer:** Thatgfsj

## TL;DR

* **Local secrets** are encrypted at rest with the OS keystore
  (DPAPI on Windows, Keychain on macOS, libsecret on Linux) — Phase 3.
* **The webview is sandboxed** via Tauri's capability system and a
  strict Content-Security-Policy — Phase 2.
* **Auto-update artifacts** are signed with an ed25519 keypair;
  the public key is compiled into the shell binary — Phase 1.
* **No telemetry, no phone-home.** The only outbound network
  traffic is the user's own LLM API calls.
* **Panics and React errors** are written to a persistent log
  file so users can attach a stack trace to a bug report —
  Phase 2.

## Reporting a vulnerability

**Do not** file a public GitHub issue for security vulnerabilities.

Email `security@flowntier.dev` (the address is reserved but
not yet provisioned; until it's set up, open a GitHub issue
labelled `security` instead). Include steps to reproduce,
version affected, and your assessment of severity. Expect an
acknowledgement within 72 hours.

## Threat model (v0.4)

### In scope

**[P2] Content Security Policy** restricts what the React app
can load and connect to:

```
default-src 'self';
script-src 'self';
style-src 'self' 'unsafe-inline';   /* Vite injects inline styles */
font-src 'self' data:;
img-src 'self' data: blob:;
connect-src 'self' ipc: https://ipc.localhost
  https://api.openai.com
  https://api.anthropic.com
  https://generativelanguage.googleapis.com
  https://api.deepseek.com
  https://api.moonshot.cn
  https://open.bigmodel.cn
  https://api.siliconflow.cn;
frame-src 'none';
object-src 'none';
base-uri 'self';
form-action 'none'
```

In **dev mode** (`pnpm tauri:dev`) the CSP is relaxed to allow
HMR over `ws://localhost:1421` and Vite's `unsafe-eval`. The dev
CSP is set via `app.security.devCsp` so production builds never
see it.

**Tauri's built-in CSP augmentation** is left enabled
(`dangerousDisableAssetCspModification: false`). At build time
Tauri parses every emitted JS/CSS asset and injects per-asset
nonces into the CSP, so `script-src 'self'` becomes
`script-src 'self' 'nonce-XXXX'` for each page load. Disabling
this would be more permissive; we don't.

**Tauri capabilities** (`apps/desktop/src-tauri/capabilities/default.json`)
are the second layer: even if a CSP bug let an attacker run
arbitrary JS in the webview, the attacker can only call the
Tauri commands we explicitly allow (`shell:allow-open`,
`updater:default`, plus a small set of `core:*` defaults).
There is **no** `core:fs:*`, no `core:path:*`, no `dialog:*`
in the webview capabilities.

**[P2] Persistent logging** writes to a daily-rolling file
under `<data_dir>/logs/flowntier.log.YYYY-MM-DD`. The same
file receives:

* Every `tracing::*!` call from the Rust shell (including
  `tauri-plugin-updater` plugin events).
* Every React error forwarded via the `log_frontend_error`
  Tauri command.
* Panics — captured by a `std::panic::set_hook` that writes
  both to the daily log file AND to a one-shot
  `panic-YYYYMMDD-HHMMSS.log` with a force-capture backtrace.

The user can:
* Click "📋 复制日志" on the React error screen (ErrorBoundary)
  to copy the last 50 lines of console output + the React
  error + component stack to the clipboard.
* Attach the log file in a GitHub issue.

**[P2] ErrorBoundary** catches every uncaught exception in the
React tree and renders a dedicated crash screen with
"复制日志 / 重启应用 / 上报问题" buttons. Without this, a
single thrown error would blank the entire WebView2 process —
the failure mode that hurt Phase 0 testing.

**[P2] Graceful startup error dialog.** If AppState::build()
fails (data dir unwritable, SQLite migration error, etc.),
the Tauri shell shows a native error dialog (Win32 MessageBox /
NSAlert / GTK) containing the error message and the log file
path. The user is no longer faced with a silent splash-then-
disappear on launch failure.

### Out of scope (v0.4)

* **Windows code signing** — SmartScreen will warn
  "Unknown publisher"; users click "More info → Run anyway".
  Deferred to v0.5.
* **macOS notarization** — Gatekeeper blocks the `.dmg` on
  first launch. Users right-click → Open.
* **Sandbox escapes** — the Tauri shell spawns the Rust
  sidecar (`flowntier-runtime`) with the user's ambient
  permissions. Malicious input that escapes the agent loop
  can run arbitrary commands. The threat is mitigated by
  the agent loop's tool capabilities (`ToolContext::read_only()`,
  `no_modify()`, `network_off()`) but is not eliminated.
* **Replay / MITM** on the named pipe `\\.\pipe\flowntier_runtime`.
  On a single-user Windows machine the pipe ACL is enforced by
  the OS; on multi-user systems a local user could in theory
  attach to the pipe. Out of scope for v0.4 because the
  desktop app assumes single-user; multi-user hardening is v0.5+.

## Cryptographic posture

| Use                  | Algorithm     | Key location                                   |
|----------------------|---------------|------------------------------------------------|
| Secret encryption    | AES-GCM-256   | OS keystore (DPAPI/Keychain/libsecret) — Phase 3 |
| Auto-update signing  | ed25519       | `TAURI_SIGNING_PRIVATE_KEY` GitHub secret — Phase 1 |
| TLS to LLM providers | TLS 1.2+      | System trust store                             |
| Local log integrity  | (none)        | Logs are plaintext JSON; trust relies on file ACL |

## Known limitations

* The webview is **single-process** — a renderer crash kills the
  whole app. Mitigated by the ErrorBoundary reload button.
* Logs are plaintext JSON. If a user attaches `flowntier.log`
  to a public GitHub issue, **request/response bodies** are
  redacted via the `logging.redact` patterns
  (`*KEY*`, `*TOKEN*`, `*SECRET*`, `*PASSWORD*`, `*AUTH*`),
  but API URLs and request IDs are not.
* The `error.action.copyLogs` button uses
  `navigator.clipboard.writeText()` which requires a secure
  context. In Electron-style webviews this is satisfied; in
  regular browser preview of the React app it's not and the
  fallback `window.prompt` is used.
* The panic hook's backtrace uses `Backtrace::force_capture()`
  which requires RUST_BACKTRACE=full OR a debug build. In
  release without that env var, the backtrace will say
  "<empty backtrace>". Documented; user can set
  `FLOWNTIER_LOG=debug` to see more.

## Process

When adding a new outbound network call:
1. Add the destination host to the CSP `connect-src` list.
2. If the call needs cookies / auth headers, document the
   storage location (must be one of: env var, OS keystore,
   runtime config — never plain disk).
3. Get Thatgfsj's approval before merging.

When adding a new Tauri command:
1. Implement the handler in `crates/pipe-server/src/handlers.rs`
   (preferred) or `apps/desktop/src-tauri/src/lib.rs` (only
   when the command must run before AppState is ready).
2. Register the command in `invoke_handler!`.
3. Add the JS wrapper under `apps/desktop/src/lib/` or
   inline in the consumer if it's only used once.
4. Document any new IPC surface in
   `history/docs/AGENT_PROTOCOL.md` (when that doc moves back
   from history/).

When adding a new dependency:
1. Check that the dep's license is MIT / Apache-2.0 / BSD-3.
2. Run `cargo deny check` and `pnpm audit` locally.
3. CI runs both on every PR (see `.github/workflows/ci.yml`).

## What changed in v0.4

* **Added** [P2]: Persistent logging (daily rolling file +
  panic hook).
* **Added** [P2]: React ErrorBoundary with copy / restart /
  report actions.
* **Added** [P2]: Graceful startup error dialog (native
  MessageBox with log path).
* **Added** [P2]: Content Security Policy (strict in
  production, relaxed in dev for HMR).
* **Added** [P1]: Auto-update signature verification (ed25519).
* **Added** [P1]: Signing key rotation procedure
  (see `docs/INSTALLER.md §6`).
* **Removed** [P0]: All Python runtime + Claude Code adapter
  (deleted in commit 9527436; see `history/docs/V03_DELETIONS.md`).
* **Removed** [P0]: `ACO_*` env var prefix; renamed to
  `FLOWNTIER_*`.
* **Changed** [P0]: Brand `Agent Company OS` → `Flowntier`
  throughout the codebase; `tools/replace_in_files.py`
  documents the safe text-replace procedure.
* **Removed** [P1]: Python sidecar build (PyInstaller) from
  release CI; v0.4 ships a single Rust binary tree.
