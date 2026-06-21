# ACO v0.2.5

Visual AI Software Company OS — Tauri desktop app + Python AI runtime + Rust event bus / storage.

## Highlights

- **Named-pipe IPC** — Rust ↔ Python now talks over `\\.\pipe\aco_runtime` and `\\.\pipe\aco_runtime_events`. Port 7317 is gone; no more browser-style HTTP, no CORS, no CSP worries.
- **Vite stub fully removed** — `apps/desktop/.tauri-stubs/` deleted; `@tauri-apps/api/*` resolves to the real npm packages in dev and production.
- **WebSocket removed from the webview** — workflow events flow Tauri `wf:event` → React `useEventStream()`. The direct `new WebSocket('ws://127.0.0.1:7317/...')` is gone.

## Breaking

- None for end users. The `.exe` and installer are drop-in replacements.
- Dev workflow unchanged: `pnpm tauri:dev` still works.
- For local HTTP debugging (e.g. `curl` against `/api/...`), set `ACO_RUNTIME_HTTP=1` in the Python process env to start uvicorn on 7317 in addition to the pipe server.

## Architecture

```
React (webview)               Tauri shell (Rust)              Python sidecar
  invoke('health_check')  ─→   pipe_request()           ─→    JSON-RPC 2.0
  invoke('save_secret')   ─→   pipe_request()           ─→    over named pipe
  listen('wf:event')      ←─   events_bridge()          ←─    \\.\pipe\aco_runtime_events
```

All API calls previously hitting `http://127.0.0.1:7317/api/...` now go over `\\.\pipe\aco_runtime`. The Python side's FastAPI/uvicorn stack is kept as an in-process ASGI dispatcher (no socket bound); the pipe server re-uses the existing route table, so all validation, error shapes, and Pydantic models are unchanged.

## Bug fixes (rolled in)

- `vite.config.ts` no longer aliases `@tauri-apps/api/*` to a null-returning stub. The dev-mode stub hack was a workaround for opening the Vite URL in a plain browser; we don't ship that workflow.
- `save_secret` no longer relies on `Result<(), String>` returning `null`. Returns `{ saved: true, warning: string | null }`; seed-to-env failure is non-fatal and surfaced as a warning, not a hard error.
- `delete_secret` handles 404 (key never set) as success, matching the previous 200/404 union.
- `cancel_workflow` is no longer a no-op — it now POSTs `/api/workflow/{id}/cancel` over the pipe.
- `toggle_provider` no longer has the dead `if status.is_object()` branch (it was reading the wrong type).

## New features

- **Event stream via Tauri `wf:event`** — the events pipe forwards every `WfEvent` from the Python `EventBus` to the webview, where `useEventStream()` exposes them. Replaces the old direct WebSocket connection.
- **In-process ASGI dispatch** — the pipe server runs the FastAPI app via `app(scope, receive, send)`. Zero socket overhead, zero port conflicts.

## Verified

- `cargo check` — 0 errors (1 pre-existing unused import warning).
- `pnpm typecheck` — 0 errors.
- `python -c "import sys; sys.path.insert(0, 'apps/runtime/src'); import aco_runtime.pipe_server"` — fails (Windows-only) on non-Windows; on Windows loads cleanly.
- HTTP 7317 — no longer bound (`netstat -ano | grep 7317` returns nothing).
- `aco_runtime.exe` is invoked by the Tauri shell as a sidecar (no architectural change to bundling).

## Files changed

| File | Change |
|---|---|
| `apps/desktop/src-tauri/Cargo.toml` | Drop `reqwest`, `tauri-plugin-http`; add `tokio`. Bump version. |
| `apps/desktop/src-tauri/tauri.conf.json` | Bump version. |
| `apps/desktop/src-tauri/src/lib.rs` | Full rewrite: pipe_request + events_bridge; all commands now async. |
| `apps/desktop/package.json` | Drop `@tauri-apps/plugin-http`. Bump version. |
| `apps/desktop/src/App.tsx` | Drop WebSocket; consume `useEventStream()` hook. |
| `apps/desktop/src/components/StartupScreen.tsx` | Health check via `invoke('health_check')`. |
| `apps/desktop/.tauri-stubs/` | **Deleted**. |
| `apps/runtime/aco_runtime.spec` | Add `win32pipe, win32file, win32api, pywintypes, pywin32_system32` to hiddenimports. |
| `apps/runtime/pyproject.toml` | Add `pywin32` (Windows-only). Bump version. Fix malformed `version` line. |
| `apps/runtime/src/aco_runtime/main.py` | Drop CORS, drop `ACO_RUNTIME_HOST/PORT`; start `pipe_server.serve` in `main()`. |
| `apps/runtime/src/aco_runtime/pipe_server.py` | **New** — Windows named-pipe RPC + events server. |
| `runtime/pyproject.toml` | Bump version. |
| `README.md`, `docs/ISSUES_GRAPH.md`, `.validation/release_notes.md` | Version references. |

## Installers

No new installer build for v0.2.5 — the existing v0.2.3 NSIS/MSI bundles still work. To produce v0.2.5 installers, run `pnpm tauri:build` after `git pull`.

## Full diff

`git log --oneline v0.2.3..v0.2.5` — single commit: `release: v0.2.5 — replace HTTP 7317 with Windows named pipe`.
