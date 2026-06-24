# Plugin Spec

> Plugin interface for Agent Company OS

**Version:** v0.1 RFC
**Status:** Draft
**Author:** Thatgfsj
**Supersedes:** PROJECT_SPEC.md §11
**Last updated:** 2026-06-18

---

## 1. Goals

1. **Extend ACO without recompiling.** Anyone can add a plugin.
2. **Capability-based sandbox.** A plugin gets only what it asks for.
3. **Cross-platform.** Plugins must work on Windows, macOS, Linux.
4. **Discoverable.** The runtime lists available plugins in a single registry.
5. **Safe to fail.** A buggy plugin must not crash the host.

---

## 2. What a Plugin Is

A plugin is a **self-contained directory** containing:

* A `plugin.toml` manifest
* An **entry binary** (executable) **or** a **WebAssembly module**
* Optional assets (icons, schemas, prompt templates)

The runtime **loads** the manifest, **negotiates** capabilities, and
**launches** the plugin via a well-defined IPC contract.

In v0.1, the only supported plugin type is **binary + JSON-RPC over stdio**.
WASM support is planned for v0.3.

---

## 3. Plugin Types

| Type               | Examples                              | v0.1 |
|--------------------|---------------------------------------|-------|
| Source control     | `git`, `github`                       | ✅    |
| Runtime            | `docker`, `podman`                    | ✅    |
| Dev tool           | `terminal`, `editor`                  | ✅    |
| Data source        | `database`, `figma`                   | ✅    |
| Communication      | `slack`, `discord`, `email`           | ✅    |
| AI integration     | `mcp`, `browser`                      | ✅    |
| Custom             | user-defined                          | ✅    |

---

## 4. Directory Layout

```
plugins/
  git/
    plugin.toml
    bin/
      aco-plugin-git[.exe]   # the executable
    icons/
      git.svg
    schemas/
      diff.v1.json
    tests/
      smoke.sh
  github/
    plugin.toml
    bin/
      aco-plugin-github[.exe]
    ...
```

---

## 5. Manifest — `plugin.toml`

```toml
[plugin]
id          = "git"                                # unique, kebab-case
name        = "Git"                                # human-readable
version     = "0.1.0"                              # semver
author      = "ACO Core Team"
license     = "MIT"
description = "Source control via git CLI"
homepage    = "https://github.com/aco/plugins-git"

[entry]
kind        = "binary"                             # "binary" | "wasm"
path        = "bin/aco-plugin-git"                 # relative to plugin dir
min_runtime = "0.1.0"                              # ACO version requirement

[capabilities]
# What this plugin can do. The runtime grants only what's listed.
read_filesystem   = ["read"]                       # "read" | "write" | "exec"
write_filesystem  = ["read", "write"]
network           = ["https://github.com"]
spawn_process     = ["git"]
access_env        = []                             # env var names, NOT values
access_clipboard  = false
access_keystore   = false                          # reserved; v0.2

[ipc]
protocol = "json-rpc/2.0"
transport = "stdio"

[contributes]
# What UI / agents surfaces this plugin registers. See §10.
agent_actions     = ["plugin:git:commit", "plugin:git:diff"]
workflow_phases   = []                             # v0.4
ui_panels         = []
prompts           = []
```

**Rules:**

* `id` is unique. Runtime refuses to load a duplicate.
* `version` follows semver. Major bump = breaking IPC change.
* `capabilities` must be **exhaustive** — the plugin cannot ask for more
  at runtime than declared here.
* `access_env` is a list of env var **names**, never values.
* `access_keystore` is forbidden in v0.1 (kept for future).

---

## 6. Lifecycle

```
                  ┌──────────────┐
                  │  DISCOVERED  │  (manifest read)
                  └──────┬───────┘
                         │ validate
                         ▼
                  ┌──────────────┐
                  │  VALIDATED   │
                  └──────┬───────┘
                         │ init(env, capabilities)
                         ▼
                  ┌──────────────┐
                  │ INITIALIZED  │
                  └──────┬───────┘
                         │ enable
                         ▼
                  ┌──────────────┐
                  │   ENABLED    │  ◀── normal operation
                  └──────┬───────┘
                         │ disable
                         ▼
                  ┌──────────────┐
                  │  DISABLED    │
                  └──────┬───────┘
                         │ unload
                         ▼
                  ┌──────────────┐
                  │  UNLOADED    │  (terminal)
                  └──────────────┘
```

| Transition         | Trigger                | Side effects                              |
|--------------------|------------------------|-------------------------------------------|
| `DISCOVERED` → `VALIDATED`   | manifest parse ok | Capability check; signature check (v0.2) |
| `VALIDATED` → `INITIALIZED`  | user/admin enables | Spawn process, send `initialize` RPC     |
| `INITIALIZED` → `ENABLED`    | first call from agent | Ready for traffic                       |
| `ENABLED` → `DISABLED`       | user/admin disables | Stop accepting calls; existing calls finish |
| `DISABLED` → `UNLOADED`      | user/admin unloads   | Kill process                            |
| any → `UNLOADED`             | fatal IPC error       | Mark plugin broken; surface to user     |

---

## 7. IPC Contract

### 7.1 Transport

* **stdio** for binary plugins (newline-delimited JSON-RPC 2.0)
* **WASM linear memory + host imports** for WASM plugins (v0.3)

### 7.2 Methods (v0.1)

| Method                | Direction     | Purpose                              |
|-----------------------|---------------|--------------------------------------|
| `initialize`          | host → plugin | Hand the plugin its config + caps   |
| `shutdown`             | host → plugin | Graceful exit                       |
| `list_actions`        | host → plugin | List calls the plugin exposes        |
| `action`              | host → plugin | Execute a named action               |
| `event`               | plugin → host | Plugin reports something happened    |
| `log`                 | plugin → host | Log line to host's console          |
| `progress`            | plugin → host | Progress update (0.0–1.0)            |

### 7.3 `initialize` request

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "initialize",
  "params": {
    "runtime_version": "0.1.0",
    "plugin_id": "git",
    "config": {
      "default_branch": "main"
    },
    "granted_capabilities": {
      "read_filesystem":  ["read"],
      "write_filesystem": ["read", "write"],
      "network":          ["https://github.com"],
      "spawn_process":    ["git"],
      "access_env":       ["GIT_AUTHOR_NAME", "GIT_AUTHOR_EMAIL"]
    },
    "workspace_path": "/path/to/aco/workspace"
  }
}
```

### 7.4 `initialize` response

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "ok": true,
    "plugin_version": "0.1.0",
    "actions": [
      {
        "name": "commit",
        "description": "Stage and commit changes",
        "params_schema_ref": "schemas/commit.v1.json"
      },
      {
        "name": "diff",
        "description": "Show diff of working tree",
        "params_schema_ref": "schemas/diff.v1.json"
      }
    ]
  }
}
```

### 7.5 `action` request

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "action",
  "params": {
    "name": "commit",
    "params": {
      "message": "Add /login",
      "files":   ["src/auth/login.py"]
    }
  }
}
```

### 7.6 `action` response

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "result": {
    "ok": true,
    "data": {
      "commit_sha": "abc1234",
      "files_changed": 1
    }
  }
}
```

### 7.7 Error shape

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "error": {
    "code": -32001,
    "message": "Permission denied: write to /etc is not allowed",
    "data": {
      "capability": "write_filesystem",
      "requested": "/etc/passwd"
    }
  }
}
```

### 7.8 Error codes

| Code   | Meaning                                  |
|--------|------------------------------------------|
| -32700 | Parse error                              |
| -32600 | Invalid request                          |
| -32601 | Method not found                         |
| -32602 | Invalid params                           |
| -32603 | Internal error                           |
| -32001 | Capability denied                        |
| -32002 | Plugin not initialized                   |
| -32003 | Plugin already shutting down             |
| -32004 | User canceled                            |
| -32010 | Network error                            |
| -32011 | External process failed                  |
| -32099 | Plugin-defined (use `data` for details)  |

---

## 8. Sandboxing

### 8.1 What the host enforces

* **Filesystem:** plugin can only read/write under `workspace_path` unless
  `read_filesystem` or `write_filesystem` lists broader paths.
* **Network:** outbound only; hostnames/ports must be in `network`.
* **Process spawn:** only binaries listed in `spawn_process`.
* **Env:** only vars listed in `access_env` are visible to the plugin.
* **CPU/Memory:** enforced by the host (v0.1: process-level limits via
  `ulimit` / Job Objects; v0.3: WASM linear memory).

### 8.2 What the plugin must do

* Validate every input from the user/agent (don't trust the host blindly).
* Never read env vars not in `access_env`. (Use `os.environ` only on
  declared names.)
* Never log secrets. The host will redact `process.env` values matching
  `*KEY*`, `*TOKEN*`, `*SECRET*` from plugin logs.

### 8.3 Bypass (escape hatch)

A plugin can be marked `[capabilities] unrestricted = true` in the
manifest. This **disables** all sandbox checks. The runtime shows a
warning dialog before loading such a plugin. **Not** recommended.

---

## 9. Discovery

### 9.1 Search paths

The runtime scans these paths in order, collecting all `plugin.toml`
files found:

| OS       | Paths                                                                 |
|----------|------------------------------------------------------------------------|
| Windows  | `%APPDATA%\aco\plugins\`, `%PROGRAMDATA%\aco\plugins\`, `./plugins/`   |
| macOS    | `~/Library/Application Support/aco/plugins/`, `/Library/Application Support/aco/plugins/`, `./plugins/` |
| Linux    | `~/.config/aco/plugins/`, `/etc/aco/plugins/`, `./plugins/`            |

### 9.2 Conflicts

* Two plugins with the same `id` → first one wins; runtime logs a warning.
* Two plugins contributing the same `agent_action` → user is asked to
  pick one (or disambiguate via `plugin:<id>:<action>`).

---

## 10. Contribution Points

A plugin can register any of these, declared in `plugin.toml`:

| Key                | Type   | Used by                                |
|--------------------|--------|----------------------------------------|
| `agent_actions`    | list   | Chief Agent's available tools          |
| `workflow_phases`  | list   | Adds a new phase to [WORKFLOW_SPEC.md](./WORKFLOW_SPEC.md) |
| `ui_panels`        | list   | Adds a side panel to the UI (v0.2)     |
| `prompts`          | list   | Injects a prompt template              |

The runtime merges these into the global registry on plugin enable.

### 10.1 Example: `git` plugin's actions

```toml
[contributes]
agent_actions = [
  "plugin:git:status",
  "plugin:git:diff",
  "plugin:git:commit",
  "plugin:git:branch",
  "plugin:git:log"
]
```

Chief Agent sees these as callable tools in its planning step. The
runtime translates them to `action` calls on the plugin.

---

## 11. Failure Modes

| Failure                              | Host behavior                              |
|--------------------------------------|--------------------------------------------|
| Plugin binary missing                | Mark broken; surface to user               |
| Plugin crashes                       | Restart once; if still fails, disable      |
| Plugin returns malformed JSON        | Send `parse_error`; disable after 3 strikes |
| Plugin exceeds memory/time budget    | Kill; mark broken                          |
| Plugin asks for capability not granted | Send `capability_denied`; plugin decides  |
| Plugin hangs (no response > 60s)     | Cancel; mark `slow`                        |

---

## 12. Authoring a Plugin (Quickstart)

1. `mkdir plugins/my-plugin && cd plugins/my-plugin`
2. Write `plugin.toml` (see §5)
3. Write a binary that speaks JSON-RPC over stdio (any language).
   Reference impls in Rust and Python are in `plugins/_examples/`.
4. `cargo build --release` (Rust) / `pip install .` (Python) / `go build`
5. Drop the binary into `plugins/my-plugin/bin/`
6. Restart ACO. The plugin appears in Settings → Plugins.

The reference `git` plugin implementation lives in
`plugins/git/` and is a fully worked example.

---

## 13. Security Review

Plugins requesting **any** of these capabilities go through manual
review before being listed in the official registry:

* `unrestricted = true`
* `network` with a wildcard (`*`)
* `spawn_process` with a wildcard
* `access_keystore = true`

User-installed third-party plugins are **not** reviewed; they run
under the declared capabilities only.

---

## 14. Open Questions

1. Should plugins be able to spawn **sub-plugins**? (proposed: no in v0.1)
2. Should we support a **plugin marketplace**? (proposed: yes, v0.4)
3. Should plugins have a **UI thread** of their own (e.g., a settings
   panel)? (proposed: yes, v0.2, via a separate `ui` binary)

---

**RFC ends.**
