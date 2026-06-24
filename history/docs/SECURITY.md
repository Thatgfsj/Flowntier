# Security

> Threat model, secret management, sandboxing, audit

**Version:** v0.1 RFC
**Status:** Draft
**Author:** Thatgfsj
**Related:** [PLUGIN_SPEC.md](./PLUGIN_SPEC.md) · [CONFIG.md](./CONFIG.md) · [STORAGE_SPEC.md](./STORAGE_SPEC.md)
**Last updated:** 2026-06-18

---

## 1. Threat Model

ACO is a **local-first desktop application**. The trust model is:

* The user trusts the ACO binary they installed.
* The ACO binary does **not** trust:
  * Model providers (their output is text until validated)
  * Plugins (sandboxed; least-privilege)
  * Network responses in general

### 1.1 Assets

| Asset                         | Sensitivity | Where it lives                |
|-------------------------------|-------------|-------------------------------|
| User's source code            | High        | User's filesystem             |
| API keys (Anthropic, OpenAI)  | Critical    | Environment variables         |
| Workflow logs (chat history)  | Medium      | SQLite + JSONL (local)        |
| Project memory                | Medium      | SQLite (local)                |
| Plugin manifests / binaries   | Medium      | `~/.config/aco/plugins/`      |
| Cost data                     | Low         | SQLite (local)                |

### 1.2 Adversaries

| Adversary                            | Capability              | Mitigation                          |
|--------------------------------------|--------------------------|--------------------------------------|
| Malicious plugin                     | Local code execution     | Capability sandbox (PLUGIN_SPEC §8)  |
| Compromised model provider           | Returns adversarial text | JSON-Schema validation of all output |
| Network attacker (MITM)              | Reads/modifies traffic  | TLS (everywhere); cert pinning for known providers |
| Local malware with same user privs   | Reads user files         | Out of scope — user-privs is the limit |
| Prompt injection (in user-supplied data) | Influences Worker output | Worker treats external data as untrusted; no `exec()` of worker output |
| Stolen laptop                        | Reads storage            | OS-level disk encryption (user's responsibility); app-level encryption planned for v1.0 |

### 1.3 Out of scope (v0.1)

* Multi-user authentication (single-user desktop app)
* Network-level isolation
* Hardware security module integration
* FIPS compliance

---

## 2. Secret Management

### 2.1 The Rule

> **API keys live in environment variables. Nowhere else.**

The runtime refuses to start if a key is found in a config file or
in the SQLite database. The only exception is OAuth refresh tokens
for plugins, which are stored in the OS keychain
(Windows Credential Manager / macOS Keychain / libsecret), never
in SQLite.

### 2.2 Recognized env var names

| Provider        | Env var                    |
|-----------------|----------------------------|
| Anthropic       | `ANTHROPIC_API_KEY`        |
| OpenAI          | `OPENAI_API_KEY`           |
| Google Gemini   | `GOOGLE_API_KEY` / `GEMINI_API_KEY` |
| Kimi            | `MOONSHOT_API_KEY`         |
| MiniMax         | `MINIMAX_API_KEY`          |
| DeepSeek        | `DEEPSEEK_API_KEY`         |
| SiliconFlow     | `SILICONFLOW_API_KEY`      |
| OpenRouter      | `OPENROUTER_API_KEY`       |
| Ollama          | _(none; local)_            |
| LM Studio       | _(none; local)_            |
| Custom          | `ACO_PROVIDER_<NAME>_API_KEY` |

The runtime logs which providers **have** a key (not the keys
themselves) on startup. Missing keys are reported per-provider.

### 2.3 .env handling

* `.env` files in the project dir are loaded **only** if the file's
  permissions are 0600 (Unix) or owned by the current user with
  no inheritance (Windows).
* `.env` is in `.gitignore`; `.env.example` is committed.

### 2.4 No logging of secrets

The `tracing` and `Loguru` configs strip values matching:

```
*KEY*   *TOKEN*   *SECRET*   *PASSWORD*   *AUTH*
```

case-insensitive, from all log lines. The redaction is in the
log-encoder layer, not in the call sites — it cannot be bypassed
by a forgotten `tracing::debug!`.

---

## 3. Plugin Sandbox

See [PLUGIN_SPEC §8](./PLUGIN_SPEC.md) for the full spec. Summary:

| Capability               | v0.1 enforcement        |
|--------------------------|--------------------------|
| Filesystem (read)        | Path allowlist            |
| Filesystem (write)       | Path allowlist + workspace-only by default |
| Network                  | Host/port allowlist       |
| Process spawn            | Binary name allowlist     |
| Environment variables    | Name allowlist (no values leaked) |
| Memory/CPU               | Process-level limits (ulimit / Job Objects) |
| IPC                      | JSON-RPC, structured      |

A plugin marked `[capabilities] unrestricted = true` bypasses all
checks after a user-visible warning. **Discouraged.**

---

## 4. Network

### 4.1 Outbound

* All provider traffic uses **TLS 1.2+** (no HTTP fallback).
* For known providers, the runtime **pins** the CA cert chain
  (not the leaf) to defeat rogue CAs.
* Per-plugin network allowlist — a plugin cannot reach hosts the
  user didn't approve.

### 4.2 Inbound

* **None.** ACO does **not** open any listening port by default.
* The Tauri webview only loads from the bundled `tauri://` (or
  `https://tauri.localhost`) origin.
* If a future feature needs inbound (e.g., mobile companion),
  it will be opt-in, on a separate allowlisted port, with a
  user-set shared secret. v0.1 has no such feature.

### 4.3 DNS

* Provider hostnames are resolved at use time. No DNS prefetch.
* Plugins cannot make DNS queries except through the host's
  allowlist check.

---

## 5. Worker Output Validation

A Worker's `TASK_RESULT` is **text** until proven otherwise.
The runtime never `eval`s, `exec`s, or otherwise interprets it.

### 5.1 What the runtime does

* Validates the JSON envelope against the schema
  ([AGENT_PROTOCOL §3](./AGENT_PROTOCOL.md)).
* Validates every `files_modified.path` against the task's
  declared `deliverables`. Files outside that set are reported
  as **scope violations** and trigger a review issue.
* The Rust core writes the files itself; it never trusts the
  Worker process to do so.

### 5.2 What the runtime does NOT do

* It does not run code in `TASK_RESULT.summary`.
* It does not follow URLs in any agent output.
* It does not trust `TASK_RESULT.tests_run` — it re-runs tests
  via the Claude Code CLI before accepting a result.

---

## 6. Audit Log

Every state-changing operation is logged immutably.

### 6.1 What is logged

* Workflow transitions (already in `workflow_log`).
* Every provider call: model, token count, hash of system+user
  prompt (for de-dup; not the content), latency, status.
* Every plugin call: plugin id, action, params (redacted),
  result status.
* Every file write by the runtime: path, lines added/removed.
* Every config change.

### 6.2 What is NOT logged

* Full prompt content (privacy + size; `prompts` table is opt-in).
* API key values.
* Plugin output bodies (only status + size).

### 6.3 Access

* Audit log is in `$ACO_DATA/audit/<yyyy-mm>.jsonl` (append-only).
* The UI exposes a filtered, paginated view (Settings → Audit).
* A "Reveal" button for advanced users shows the full content
  for a selected row, with a one-time warning.

---

## 7. User Confirmation for Sensitive Operations

The runtime asks the user before:

* Writing a file outside the workspace path
* Running a plugin that requests `unrestricted = true`
* Deleting a workflow's logs (vs. archiving)
* Modifying `aco.toml` providers.toml, or router.toml
* Installing a new plugin (vs. just enabling a discovered one)
* Connecting to a custom OpenAI-compatible endpoint

Confirmation is a modal with: **what**, **why**, **how to revert**.

---

## 8. Supply Chain

### 8.1 Dependencies

* **Rust:** `cargo audit` runs in CI; new advisories fail the build.
* **Python:** `pip-audit` (or `uv pip audit`) in CI.
* **TS:** `pnpm audit` in CI; critical advisories fail.
* **Lockfiles committed:** `Cargo.lock`, `pnpm-lock.yaml`,
  `uv.lock` — never regenerated without PR review.

### 8.2 Build provenance

* Releases are built by GitHub Actions (matrix: win/mac/linux).
* Build artifacts are signed with `cosign` (keyless, OIDC) and
  attached to the GitHub release.
* SBOM (CycloneDX) is generated per release and attached.

### 8.3 Runtime integrity

* App binary is **not** code-signed in v0.1 (planned v1.0).
* Auto-update is opt-in and **verifies the signature** of any
  update payload (planned v0.4).

---

## 9. Reporting a Vulnerability

* **Email:** security@aco.local _(TBD; placeholder until domain exists)_
* **PGP key:** _(TBD)_
* **Response SLA:** 72 hours acknowledgement, 14 days triage,
  90 days disclosure.
* **Hall of fame:** published on the security page.

---

## 10. Compliance

ACO makes **no** compliance claim in v0.1 (no SOC 2, no GDPR-DPA,
no HIPAA). It is a local tool; the user owns all data.

For v1.0, consider:

* GDPR data export (already supported via Settings → Export)
* Right-to-be-forgotten = "Delete this workflow" + "Forget project
  memory key"
* Cookie-free, telemetry-free (default)

---

## 11. Open Questions

1. Should we add a **read-only mode** for triaging a project
   without mutating it? (proposed: yes, v0.3)
2. Should we integrate with the OS keychain **by default** for API
   keys (instead of env vars)? (proposed: env vars are simpler; OS
   keychain is a v0.4 option)
3. Should we ship a **`--harden` flag** that disables plugins,
   custom endpoints, and other risk surfaces? (proposed: yes, v0.3)

---

**RFC ends.**
