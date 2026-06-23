# v0.3 Deletion Manifest — COMPLETE

> **Status: ✅ all phases done (commit applied on 2026-06-23).**
> This document is kept as a historical record of what was
> removed and why. For the post-deletion state of the
> workspace, see `docs/ROADMAP.md`.

---

## Phase 1: Agent runtime — DONE

Files removed:

| Path | Reason | Replaced by |
|------|--------|-------------|
| `apps/runtime/` | Tauri sidecar binary — Python FastAPI/uvicorn entry | `crates/pipe-server` (Rust) |
| `runtime/` | Python package (the actual library) | `crates/agent-core` |
| `crates/claude-adapter/` | PTY wrapper around the `claude` CLI | `crates/agent-core/src/provider/` (HTTPS) + `crates/agent-core/src/tools/` (in-process) |
| `crates/claude-adapter/Cargo.toml` deps (`portable-pty`, `tokio-util io`) | no consumers after removal | n/a |

## Phase 2: Workspace cleanups done at the same time

- `Cargo.toml` workspace `[members]` no longer lists `crates/claude-adapter`.
- `crates/tauri-core/Cargo.toml` no longer depends on `crates/claude-adapter`.
- `apps/desktop/src-tauri/Cargo.toml` no longer depends on `crates/claude-adapter`.
- `crates/tauri-core/src/lib.rs` no longer holds an
  `Arc<dyn claude_adapter::ClaudeRunner>` field on `AppState`
  — the only consumer was the smoke-print in `src/bin/aco.rs`,
  which now ends at the EventBus line.
- `docs/V03_DELETIONS.md` itself rewritten in past tense.

## Verification

Before committing the deletion, `cargo build --workspace`
was green after the `claude-adapter` dep removals. The
post-deletion state is the same green:

```
$ cargo build --workspace       → 0 errors, 0 warnings
$ cargo test  --workspace       → 60 passed, 0 failed
$ cargo clippy --all-targets     → 0 errors, 0 warnings
$ pnpm tsc  --noEmit            → 0 errors
```

## Why we kept `crates/claude-adapter/` until now

Two reasons:

1. **In-flight edits.** An external agent was concurrently
   modifying the same workspace. The commit `bb69b8f`
   (fourth-pass cleanup) added the `crates/claude-adapter`
   entry to `crates/tauri-core/Cargo.toml`'s comment-as-cohabit
   state, which would have been confusing to delete while
   that other agent was active.

2. **Acceptance verification.** We needed both runtimes on the
   dev box long enough to verify that `crates/pipe-server`
   answers the same JSON-RPC shape as the Python version, and
   that the ChatZone flows through `crates/agent-core`
   produce the same `wf:event` payloads. Both are now
   validated by `docs/ACCEPTANCE_v0.3.md` and
   `docs/ACCEPTANCE_v0.3_LEDGER.md`.

## Migration notes for anyone who still has the old crates locally

```bash
# After this commit lands:
git pull
cargo build --workspace
```

You should see exactly the seven workspace members listed in
`Cargo.toml`. The `apps/runtime/` directory will no longer
exist; if you have `pnpm` workflows that referenced it, the
only thing to change is the `tauri.conf.json` sidecar config
(which never used `apps/runtime/` because v0.3 already routed
everything through `crates/pipe-server` over the named pipe).
