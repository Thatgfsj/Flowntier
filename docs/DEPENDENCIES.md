# Dependencies

This document tracks the third-party dependencies used in Flowntier
and their security/license posture.

**Status (v0.4):** *placeholder*. Real content is added once we
run `cargo deny check` in CI and freeze the dependency tree.

## Rust workspace

The workspace depends on the crates listed in `Cargo.toml` at the
repository root. Major direct dependencies:

| Crate          | Why we use it                          |
|----------------|----------------------------------------|
| `tauri` 2      | Desktop shell + webview host           |
| `tokio` 1      | Async runtime for the Rust core        |
| `sqlx` 0.8     | SQLite repository (compile-time checked queries) |
| `reqwest` 0.12 | HTTPS client for LLM provider APIs     |
| `eventsource-stream` | SSE parser for streaming completions |
| `clap` 4       | CLI parsing for the standalone `flowntier` binary |
| `serde` 1      | JSON / TOML serialization              |

All dependencies are MIT or Apache-2.0 / MIT dual-licensed. The full
tree lives in `Cargo.lock`.

## TypeScript

The TypeScript workspace uses pnpm. Major direct dependencies:

| Package                | Why we use it               |
|------------------------|------------------------------|
| `react` 19             | UI runtime                   |
| `@tauri-apps/api` 2    | IPC to the Rust shell        |
| `@xyflow/react` 12     | Workflow graph (DAG canvas)  |
| `@tanstack/react-query`| Server state for the React UI |
| `zustand` 5            | Client state for the React UI |
| `framer-motion` 11     | Lightweight animations       |
| `xterm` 5              | Console zone (terminal-like) |

All dependencies are MIT.

## Security advisories

Run `cargo deny check advisories` and `pnpm audit` locally to
reproduce CI. The CI runs both on every PR.

## Adding a new dependency

1. Open an issue with the use case and a link to the upstream crate.
2. Get approval (Thatgfsj for v0.x).
3. Add the dependency to the appropriate `Cargo.toml` or
   `package.json`.
4. Run `cargo update -p <crate>` (or `pnpm add <pkg>` for TS).
5. Commit the lockfile changes separately from any feature changes.
6. Verify `cargo deny` and `pnpm audit` are clean.