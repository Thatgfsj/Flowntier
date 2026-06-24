# ACO v0.2.3

Visual AI Software Company OS — Tauri desktop app + Python AI runtime + Rust event bus / storage.

## Highlights

- **Phase 2 Complete** — Task graph visualization, plugin system, per-task console
- **Bundled Python Runtime** — No Python installation required for end users
- **WebView2Loader.dll Fix** — Clean install no longer crashes on startup
- **TypeScript Strict Mode** — All 24 strict-mode errors resolved
- **Rust Clippy Clean** — All warnings resolved

## New Features

### Plan Graph Visualization (Phase 2.4)
- React Flow DAG visualization of workflow plans
- Topological layout algorithm with role-colored nodes
- Hard (solid) / Soft (dashed) dependency edges
- Click-to-select node with detail display
- MiniMap and controls

### Plugin System (Phase 2.13-2.17)
- **Docker Plugin**: build/run/test/compose/ps/images/logs
- **MCP Plugin**: Model Context Protocol bridge (placeholder)
- **Plugins Panel UI**: Plugin list, action selection, args input, result display
- 5 built-in plugins: echo, python, git, docker, mcp

### Per-Task Console (Phase 2.18)
- Task-specific log filtering
- Level-based filtering (error/warn/info/debug)
- Agent attribution
- Task tree summary view

### Provider System (Phase 2.5-2.10)
- 11 providers defined: Anthropic, OpenAI, Google, Kimi, MiniMax, DeepSeek, SiliconFlow, OpenRouter, Ollama, LM Studio, Custom
- Failover chain support with retry

## Installers (Windows x64)

| Format | Size | File |
|--------|------|------|
| NSIS (setup.exe) | 39 MB | `Agent Company OS_0.2.3_x64-setup.exe` |
| MSI | 40 MB | `Agent Company OS_0.2.3_x64_en-US.msi` |

NSIS is recommended for individual users. MSI is suitable for Group Policy / SCCM deployment.

## Runtime requirements

- Windows 10/11 x64
- WebView2 Runtime (preinstalled on Windows 11)
- ~100 MB disk (includes bundled Python runtime)

## Bug fixes

- **WebView2Loader.dll missing** — Installer now bundles the DLL
- **REPAIRING state transitions** — Fixed missing transitions from REPAIRING to final_review_* events
- **TypeScript strict-mode errors** — All 24 errors resolved
- **Rust clippy warnings** — All warnings resolved
- **Event bus async test** — Fixed blocking issue in tests

## Verified

- 155 Python runtime tests passing
- All Rust tests passing
- TypeScript: 0 errors
- Clippy: 0 errors
- End-to-end demo: `python -m aco_runtime_lib demo "Write an is_prime function"`
- Windows installer: NSIS 39 MB + MSI 40 MB

## Architecture

```
AgentCompanyOS/
├── apps/desktop/          # Tauri v2 + React 19 + TypeScript
├── apps/runtime/          # Python FastAPI sidecar (bundled)
├── runtime/               # Python AI runtime library
├── crates/                # Rust crates (tauri-core, event-bus, etc.)
├── packages/              # TypeScript packages (ui, shared, etc.)
└── docs/                  # RFCs and specifications
```

## Full diff

`git log --oneline v0.2.2..v0.2.3` — includes Phase 2 completion, plugin system, plan visualization, and build fixes.
