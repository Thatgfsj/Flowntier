# FAQ

> Frequently asked questions about Flowntier

**Version:** v0.4
**Status:** Polished for first user release
**Last updated:** 2026-06-25

---

## General

### What is Flowntier?

An **AI Software Company Operating System**. A desktop app where
specialized AI agents (Chief, Critics, Workers) collaborate through
an 8-phase workflow to ship software. The IDE is the visualization
layer; the workflow is the product.

### Is Flowntier an AI IDE?

**No.** Flowntier does not edit code directly. The user writes nothing.
The Chief delegates to Workers, who use Claude Code (or a future
adapter) to do the actual edits. The user reviews, approves, and
steers.

### Why not just use Claude Code / Cursor / Devin?

* **Claude Code** is one CLI. Flowntier is a **team** of agents with
  different roles, planning, review, and repair loops.
* **Cursor** is a code editor. Flowntier is the operating system
  *around* an editor.
* **Devin** is one agent. Flowntier's design philosophy is that **no
  single agent owns the whole project** — only the Chief does,
  and even the Chief doesn't write code.

### Why Tauri, not Electron?

Smaller binary (no bundled Chromium), Rust core lets us reuse the
backend crates, and better security defaults (no node integration
in renderer).

### Why Python for the AI runtime?

The runtime is I/O-bound (network calls, PTY I/O, event dispatch).
Python's async story is mature, the AI/ML ecosystem is Python-first,
and prompt iteration is faster without a compile step. Perf-critical
parts (FS, IPC, SQLite) stay in Rust.

---

## Architecture

### How do agents communicate?

Through the **Chief**, who is the only hub. Workers and Critics
never talk to each other. See
[AGENT_PROTOCOL.md](./AGENT_PROTOCOL.md).

### What's the role of the Chief Agent?

The Chief owns the **entire project context**. It:
1. Understands the user's request
2. Produces a plan
3. Dispatches tasks to Workers
4. Asks Critics to review
5. Repairs failed tasks
6. Reports back to the user

The Chief is the **only** agent that sees the big picture.

### Why two critics? Why not one?

* **Critic A** focuses on bugs, runtime errors, edge cases,
  backend logic.
* **Critic B** focuses on architecture, maintainability, style.

They review **independently** so one can't anchor the other. A
single agent doing both tends to focus on whatever it found first.

### Can a Worker talk to another Worker?

**No.** This is enforced at the runtime layer
([AGENT_PROTOCOL §7](./AGENT_PROTOCOL.md)). It's a feature, not a
limitation: peer-to-peer communication causes context pollution,
token explosion, and inconsistent implementations.

### Can I customize the agent prompts?

Yes, in v0.3 ("house style" overrides per project). For v0.1,
prompts live in `prompts/` and are versioned.

---

## Workflow

### How long does a typical workflow take?

* Simple feature (1–3 tasks): 5–15 min
* Medium feature (5–10 tasks): 20–60 min
* Large refactor (15+ tasks): 1–4 h

Wallclock is dominated by model latency, not orchestration.

### What happens if a Worker fails?

The Chief sends a `REPAIR_REQUEST` with the Critic's specific
issues. After 3 failed repair loops, the workflow escalates to
the user.

### Can I cancel a workflow?

Yes. Click "Stop" or hit `Cmd/Ctrl+.`. The current state finishes
its unit of work, then transitions to `ABORTED`.

### Can I resume after a crash?

Yes. On startup, Flowntier scans `workflows/*.jsonl` and offers
**Resume** / **Discard** / **Inspect** for any incomplete run.

### What if a plan is bad?

Critics review the plan before workers start. If they find
problems, the plan goes back to the Chief for revision (up to 3
times). If the Chief can't fix it, the workflow fails and the
user is asked to provide more guidance.

---

## Providers & Models

### Which models are supported?

In v0.1: Anthropic, OpenAI, Google Gemini, Kimi, MiniMax, DeepSeek,
SiliconFlow, OpenRouter, Ollama, LM Studio, and any OpenAI-compatible
endpoint. See [PROVIDER_SPEC §2](./PROVIDER_SPEC.md).

### How much does it cost?

Depends on the models you pick. A typical 10-task workflow with
Claude Opus as Chief and MiniMax M3 as Worker is **$0.50 – $2.00**.
The cost dashboard (v0.2) shows real numbers.

### Can I use local models?

Yes. Ollama and LM Studio are first-class providers. Vision and
tool-calling may be limited depending on the local model.

### Can I use my own OpenAI-compatible API?

Yes. Add an entry to `providers.toml` with `type = "openai_compat"`,
set `base_url` and `api_key_env`, and add your model specs.

### What if my provider rate-limits?

The router has a per-model token-per-minute limit. When exceeded,
tasks wait in `PENDING` until the window clears. See
[TASK_GRAPH §5](./TASK_GRAPH.md).

---

## Security & Privacy

### Does Flowntier send my code anywhere?

Only to the LLM providers you configure. Flowntier does not have a
telemetry endpoint. All your code, plans, and history stay on
your machine.

### Where are my API keys stored?

In **environment variables only**. Never in any file. The runtime
refuses to start if it finds a key in a config file.

### Is my workflow data encrypted at rest?

Not in v0.1. It will be in v1.0 (AES-256-GCM with Argon2id key
derivation). For v0.1, use OS-level disk encryption (FileVault,
BitLocker, LUKS).

### Can a plugin escape its sandbox?

A plugin marked `[capabilities] unrestricted = true` can.
A normal plugin is constrained by the manifest's filesystem,
network, and process allowlists. See [PLUGIN_SPEC §8](./PLUGIN_SPEC.md).

### What about prompt injection?

A Worker's `TASK_RESULT` is text until validated. The runtime
never `eval`s or `exec`s it. Files written are validated against
the task's declared `deliverables`. Any scope violation triggers
a review issue.

---

## Plugins

### How do I write a plugin?

See [PLUGIN_SPEC §12](./PLUGIN_SPEC.md). The reference `git`
plugin in `plugins/git/` is a fully worked example.

### What languages can plugins be written in?

Any language that can speak JSON-RPC over stdio. Reference impls
in Rust and Python are in `plugins/_examples/`.

### Can a plugin call back into Flowntier?

Yes, via the `event` JSON-RPC method (e.g., to log to the
console). It cannot read the workflow state — that's the host's
responsibility.

### When are MCP plugins supported?

MCP is supported as a **first-class plugin** in v0.1. The
`mcp` plugin bridges MCP servers into Flowntier's plugin model.

---

## Roadmap

### When is v1.0?

Target 16–24 weeks after v0.5 ships (Q1 2027 estimate, depends on
audit + freeze). See [ROADMAP.md](./ROADMAP.md).

### Will Flowntier be open source?

The plan is **yes**, MIT-licensed. The repo is public at
`https://github.com/Thatgfsj/Flowntier`.

### Will there be a cloud version?

Not in v1.0. Flowntier is local-first. A cloud-hosted version is on
the post-v1.0 roadmap.

### Will there be a mobile / web version?

Not in v1.0. The v0.1 minimum screen size is 768 px. Web and
mobile are post-v1.0.

### Can I contribute?

Yes. The contribution model is RFC-driven: open an issue, draft
a new RFC (or amend an existing one), discuss, implement. See
[CONTRIBUTING.md](../CONTRIBUTING.md).

---

## Troubleshooting

### "Provider X has no API key"

Set the env var listed in `providers.toml` under `api_key_env`,
or disable the provider with `enabled = false`.

### "Workflow stuck in PENDING"

Most likely a rate-limit. Check the bottom console for
`rate_limited` events. Increase `max_tokens_per_minute` in
`flowntier.toml` or wait.

### "All repair loops exhausted"

The Critic found issues the Worker couldn't fix in 3 attempts.
Open the workflow, review the issues, and either:
* Refine the task assignment (add a constraint to the original
  task) and re-run
* Click "Rewrite" to have the Chief re-plan that sub-graph

### "Storage is full"

Workflow JSONL and SQLite grow over time. Settings → Storage →
"Compact" runs VACUUM and prunes old `usage` / `prompts` rows
per the retention policy.

### "Flowntier won't start"

In v0.4 the Tauri shell shows a native error dialog when
`AppState::build()` fails (data dir unwritable, SQLite
migration error, etc.). The dialog includes:

* The error message
* The full path to the diagnostic log
  (`%APPDATA%\flowntier\logs\` on Windows,
  `~/.config/flowntier/logs/` on Linux)
* A pre-filled GitHub issue URL

If the dialog doesn't appear (e.g. the shell is so broken it
can't draw), inspect the log file directly. Every Rust panic +
every React error + every tracing event lands in the same
`flowntier.log.YYYY-MM-DD` file.

---

## v0.4 user questions

### "SmartScreen says 'Unknown publisher'"

The v0.4 installer is **unsigned**. We deferred code signing
to v0.5 (an EV cert costs $300–500/year and we'd need to
verify the macOS notarization story at the same time). For
now: click **More info → Run anyway**. The signature
verification on auto-update artifacts *is* in place (ed25519),
so updates from GitHub Releases are still verified — only the
initial install is unsigned.

### "Where are my API keys stored?"

AES-256-GCM ciphertext in
`%APPDATA%\flowntier\storage.sqlite` (Windows) or
`~/.config/flowntier/storage.sqlite` (Linux). The encryption
key lives in your OS keystore (DPAPI on Windows, Keychain on
macOS, libsecret on Linux). The plaintext never touches disk.

To verify: open the SQLite file with any tool, query the
`secret` table, and confirm every row has a non-empty
`ciphertext` blob but no `value` / `plaintext` column. (There
isn't one — it's a compile-time guarantee from the type
system.)

### "How do I add my own relay station / private gateway?"

Settings → Providers → "添加自定义". Fill in:

* **Name** — human label
* **Base URL** — must start with `https://` or `http://`
* **Kind** — `openai-compatible` (most relay stations) or
  `anthropic-compatible` (for Anthropic-protocol gateways)
* **Default model** — the model id the agent loop uses when
  no per-role override is set
* **API key** — encrypted the same way as preset keys

The provider is registered with a ULID id; you can refer to
it in `flowctl` (v0.5) or from the Settings UI.

### "How do I update the app?"

Flowntier checks GitHub Releases on every launch. When a
newer version is available, the TopBar shows a "⬆ 升级 vX.Y.Z"
banner. Click it to download + install + restart (Windows
NSIS in-place upgrade; Linux .deb in-place). Updates are
signed with ed25519; signature mismatch blocks the install.

### "How do I report a bug?"

Three ways, in order of preference:

1. **In-app**: trigger the error, click **🐛 上报问题** on
   the ErrorBoundary screen. Opens a pre-filled GitHub issue.
2. **Settings → 关于 → 上报问题**: same flow, no error needed.
3. **Manual**: [github.com/Thatgfsj/Flowntier/issues/new](https://github.com/Thatgfsj/Flowntier/issues/new)
   with `bug` label. Attach `%APPDATA%\flowntier\logs\flowntier.log.<date>` (Windows)
   or `~/.config/flowntier/logs/flowntier.log.<date>` (Linux).

The maintainer responds within 72 hours. Security issues:
email `security@flowntier.dev` — do not file public issues.

### "Why does the first launch show a wizard?"

The Welcome screen (`first_run=true` in the kv table) appears
once. It walks you through:
1. Pick a provider (or skip and add one later)
2. Try a sample workflow ("implement POST /auth/login")
3. Enter the workspace

Dismissing once sets `first_run=false`; never shown again.
To re-trigger (e.g. for a demo), delete the key from the kv
table:

```sql
DELETE FROM kv WHERE k = 'first_run';
```

### "What about macOS?"

Deferred to v0.5 per chairman directive. The v0.4 release
matrix is `windows-latest` + `ubuntu-latest` only. The macOS
build path is wired in `release.yml` (commented out as a
reference) and the `keyring` crate's `apple-native` feature is
already enabled so the code compiles when macOS support
re-enables.

---

**Have a question not answered here? Open an issue on GitHub.**
