# ACO v0.2.2

Visual AI Software Company OS — Tauri desktop app + Python AI runtime + Rust event bus / storage.

## Highlights

- **Per-agent timeouts** (`feat(orchestrator)`) — `OrchestratorOptions` now carries `*_timeout_seconds` for every role; the worker is wrapped in `asyncio.timeout` so a misbehaving local 1B model can no longer self-lock a workflow.
- **Encrypted secret store** — API keys live in the OS keychain (Windows Credential Manager / macOS Keychain / Linux Secret Service). A new "Secrets" tab in the Settings UI lists, edits, and reveals stored keys without ever touching `os.environ`.
- **Structured plugin system** — `Plugin` ABC + `PluginRegistry` + built-in `echo`, `python`, `git` plugins. The python plugin runs inline source in a sandboxed subprocess that does **not** inherit API keys. The git plugin defaults to read-only and requires `confirm=true` for write operations.
- **FinalReviewer agent** — a new role that runs after the Reporter drafts the delivery summary. Can PASS → DONE, REPAIR → loop back to workers, or REJECT → FAILED.
- **Plan visualization** — `GET /api/workflow/{id}/plan` returns the parsed plan + live per-task statuses, driven by a new `task_status` event variant.
- **Phase 2.1 / 2.2 / 2.3** — `plan_parser.py` (Markdown → DAG), `plan_validator.py` (cycle detection + budget), `plan_scheduler.py` (topological + fair dispatch + repair subgraph).

## Installers (Windows x64)

| Format | Size | File |
|--------|------|------|
| NSIS (setup.exe) | 3.6 MB | `Agent Company OS_0.2.2_x64-setup.exe` |
| MSI | 5.0 MB | `Agent Company OS_0.2.2_x64_en-US.msi` |

NSIS is recommended for individual users (smaller, faster install). MSI is suitable for Group Policy / SCCM deployment.

## Runtime requirements

- Windows 10/11 x64
- WebView2 Runtime (preinstalled on Windows 11; on Windows 10 the NSIS installer pulls it automatically)
- Python 3.11+ sidecar (`apps/runtime/.venv` included in the installer for dev builds)
- ~10 MB disk

## Bug fixes

- **Critic A silent PASS** — `_parse_verdict` defaulted to PASS when JSON didn't have a `verdict` field. Now returns `REPAIR` with a synthetic `parse_failure` MAJOR issue so the orchestrator surfaces the failure instead of silently approving unverified output.
- **POST /api/workflow slow first-call** — `ProviderManager.build_router()` opened fresh `httpx.AsyncClient` instances on every call (5s DNS per provider on Windows). Now cached; second POST drops from **11.1s → 0.26s**.
- **Concurrent workflow starvation** — Workflows shared the same ModelRouter + provider TCP pool; 3 concurrent runs starved each other. Added an `asyncio.Lock` so runs serialize cleanly.
- **Unknown FinalReviewer verdict** — fell through to PASS, which delivered unverified work. Now REJECT (safe default).

## Verified

- 153 runtime tests passing (`cd runtime && source .venv/Scripts/activate && python -m pytest tests/ -q`)
- End-to-end workflow (POST + /plan poll + FinalReviewer) reaches DONE
- Python plugin sandbox does NOT leak API keys (verified: `KEY=<missing>` even with keychain seeded)
- Git plugin rejects `git clean -fdx` etc. without `confirm=true`

## Deferred

- 24 pre-existing TypeScript strict-mode errors in `apps/desktop/src/simulator.ts`, `apps/desktop/src/zones/*.tsx`, and `packages/ui/src/hooks/useEventStream.ts`. Not in the release path. Tracked as 0.2.5 cleanup.
- `bus.publish` monkey-patch leak (latent; masked by workflow serialization lock). Tracked as 0.3 refactor.
- `/api/settings/secrets/{name}/reveal` requires no auth (loopback-only; should add Origin check before public exposure). Tracked as 0.3.

## Full diff

`git log --oneline v0.2.1..v0.2.2` — 24 commits including Phase 2.1–2.3, FinalReviewer, Keychain/Settings, Plugin system, and the perf + review fixes that landed during hardening.