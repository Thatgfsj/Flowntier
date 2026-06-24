# Configuration

> Configuration file layout, hierarchy, schema, validation

**Version:** v0.1 RFC
**Status:** Draft
**Author:** Thatgfsj
**Related:** [PROVIDER_SPEC.md](./PROVIDER_SPEC.md) · [STORAGE_SPEC.md](./STORAGE_SPEC.md) · [SECURITY.md](./SECURITY.md)
**Last updated:** 2026-06-18

---

## 1. Goals

1. **Predictable hierarchy** — the user always knows which file is
   overriding which.
2. **Schema-validated** — every config file is checked against a
   JSON Schema at load time. Bad config refuses to start.
3. **No secrets in any file** — see [SECURITY §2](./SECURITY.md).
4. **Hot-reload where safe** — provider list yes, workflow rules no.

---

## 2. File Hierarchy

Loaded in this order; later overrides earlier:

| #  | File                                    | Scope        | Format |
|----|-----------------------------------------|--------------|--------|
| 1  | _(built-in defaults)_                   | binary       | Rust   |
| 2  | `~/.config/aco/config.toml`             | user-global  | TOML   |
| 3  | `<project>/.aco/config.yaml`            | project      | YAML   |
| 4  | `<project>/.env` (if exists)            | project-local| dotenv |
| 5  | Environment variables                   | OS           | env    |

The project root is determined by walking up from CWD looking for
`.aco/config.yaml`.

---

## 3. `aco.toml` — Top-level Runtime Config

```toml
# ~/.config/aco/config.toml
# (also: <project>/.aco/config.yaml, YAML equivalent)

[app]
data_dir       = "~/.config/aco"      # default; OS-specific override
log_level      = "info"               # trace | debug | info | warn | error
theme          = "dark"               # dark | light (v0.2)
auto_update    = false                # opt-in
telemetry      = false                # always false in v0.1

[workflow]
max_plan_revisions   = 3
max_repair_loops     = 3
max_parallel_workers = 8
max_total_tokens     = 5_000_000
max_wallclock_secs   = 14400           # 4 h
max_user_query_wait  = 3600            # 1 h

[ui]
show_token_stream   = false           # show token-by-token; default off
show_console        = true
console_height_px   = 240
timeline_position   = "bottom"        # bottom | top

[logging]
redact  = ["*KEY*", "*TOKEN*", "*SECRET*", "*PASSWORD*", "*AUTH*"]
format  = "json"                      # json | pretty
sample_console = 1.0
sample_events  = 0.1

[storage]
retention_usage_days   = 365
retention_prompts_days = 180
backup_dir             = "~/.config/aco/backups"
backup_daily_keep      = 7
backup_weekly_keep     = 4

[security]
allow_unrestricted_plugins = false
confirm_external_writes    = true
require_signature_for_plugins = true   # v0.2
```

### 3.1 Schema validation

Loaded against `config/aco.schema.json` (bundled). On any schema
violation:

* The runtime prints the exact field + path + reason.
* The runtime refuses to start unless `--skip-config-check` is passed
  (and logs a warning if it is).

### 3.2 Hot-reload

* `app.log_level`, `app.theme`, `ui.*`, `logging.*` — hot-reloadable.
  SIGHUP on Linux/macOS, or runtime menu "Reload config" everywhere.
* `workflow.*`, `storage.*`, `security.*` — require restart.
* `providers.toml` / `router.toml` — hot-reloadable (via the
  `provider_manager` event in [ARCHITECTURE §3](./ARCHITECTURE.md)).

---

## 4. `providers.toml` — Provider Definitions

See [PROVIDER_SPEC §5.2](./PROVIDER_SPEC.md) for the full spec.
Summary:

```toml
[providers.anthropic]
type        = "anthropic"
base_url    = "https://api.anthropic.com"
api_key_env = "ANTHROPIC_API_KEY"
enabled     = true

[providers.anthropic.models.claude-opus-4-8]
display_name      = "Claude Opus 4.8"
context_window    = 200000
max_output_tokens = 32000
input_cost_mtok   = 15.0
output_cost_mtok  = 75.0
capabilities      = ["chat", "stream", "vision", "tool_call", "json_mode"]
```

Validation rules:

* `type` must be a known provider kind.
* `api_key_env` must be the name of an env var that is set (or
  the provider is marked `enabled = false`).
* Every model must have a non-empty `display_name` and a positive
  `context_window`.

---

## 5. `router.toml` — Model Routing

```toml
[defaults]
chief      = "anthropic:claude-opus-4-8"
critic_a   = "google:gemini-2-5-pro"
critic_b   = "anthropic:claude-sonnet-4-6"
worker     = "minimax:minimax-m3"
reporter   = "deepseek:deepseek-reasoner"

[fallback.chief]
chain = ["anthropic:claude-opus-4-8", "kimi:kimi-k2", "openai:gpt-5"]

[fallback.worker]
chain = ["minimax:minimax-m3", "deepseek:deepseek-chat"]

[overrides]
# task-id -> provider:model pin
"task_01HZX..." = "openai:gpt-5"
```

Validation rules:

* Every `defaults.<role>` must reference a provider:model that exists
  in `providers.toml` and is enabled.
* Every entry in a `fallback.*.chain` must satisfy the same.
* `overrides` is keyed by task ID; an unknown task ID is a warning,
  not an error (it applies when the task starts).

---

## 6. `.aco/config.yaml` — Project Config

```yaml
# <project>/.aco/config.yaml

project:
  id: my-app
  name: My Application
  memory_namespace: my-app

workflow:
  # Inherits from user-global, may override:
  max_repair_loops: 5

providers:
  # Inherits from user-global; may disable or override defaults
  google:
    enabled: true

plugins:
  required:
    - id: git
      min_version: "0.1.0"
    - id: docker
      min_version: "0.1.0"
  forbidden:
    - id: some-broken-plugin

ui:
  preferred_console_height_px: 320
```

This file is the user's **committed** project preferences. It is
git-tracked.

---

## 7. `.env` — Local Developer Overrides

```bash
# <project>/.env
ACO_PROVIDER_OPENAI_COMPAT_API_KEY=sk-...
ACO_PROVIDER_OLLAMA_BASE_URL=http://192.168.1.5:11434/v1
ACO_LOG_LEVEL=debug
ACO_DISABLE_PLUGIN_DOCKER=true
```

* Loaded only if file mode is 0600 (Unix) or owner-only (Windows).
* The `ACO_` prefix is required to avoid leaking unrelated env vars
  into the runtime.
* `.env` is in `.gitignore`. `.env.example` is committed.

---

## 8. Environment Variables

`ACO_*` variables override everything. Recognized:

| Var                              | Effect                          |
|----------------------------------|---------------------------------|
| `ACO_DATA_DIR`                   | Override `app.data_dir`         |
| `ACO_LOG_LEVEL`                  | `trace`/`debug`/`info`/`warn`/`error` |
| `ACO_THEME`                      | `dark`/`light`                  |
| `ACO_DISABLE_PLUGIN_X`           | Disable plugin by id (X)        |
| `ACO_DISABLE_PROVIDER_X`         | Disable provider by id (X)      |
| `ACO_MODEL_OVERRIDE_ROLE_X`      | Override `defaults.X` in router |
| `ACO_SKIP_CONFIG_CHECK`          | Skip JSON-Schema validation (warn only) |
| `ACO_NO_TELEMETRY`               | Forced off                      |
| `ACO_PROFILE`                    | Path to a profile file (v0.2)   |

Provider env vars (`ANTHROPIC_API_KEY`, etc.) are the only source
of API keys.

---

## 9. Schema Location

All schemas live in `packages/shared/schemas/`:

```
packages/shared/schemas/
├── aco.schema.json
├── providers.schema.json
├── router.schema.json
├── project.schema.json
└── workflow_event.schema.json
```

Bundled into the Rust binary at build time (`include_str!`).
Validated by `jsonschema` crate on load.

---

## 10. CLI Overrides

`aco run`, `aco doctor`, etc. accept flags that override config:

```bash
aco run --log-level debug --max-workers 4
aco doctor --check-providers
```

CLI flags are **highest priority** in the hierarchy, then env vars,
then files, then defaults.

---

## 11. Migration

When a config version becomes incompatible (e.g., v0.2 → v0.3):

1. On startup, the runtime detects the old version.
2. It writes the new file alongside, with renamed/added fields.
3. The user is prompted: "Migrate? [Y/n]".
4. The old file is moved to `<file>.v<n>.bak`.

Schema version lives in each file's `_schema_version` field (TOML
allows arbitrary keys; we reserve `_` prefix for metadata).

---

## 12. Open Questions

1. Should we support **JSON5** for `aco.toml` (comments + trailing
   commas)? (proposed: no, TOML is the standard)
2. Should the project config be a **single file** or a directory
   (`.aco/config/*.yaml`)? (proposed: single file in v0.1; directory
   in v0.3 if configs grow)
3. Should the runtime **print the effective config** at startup
   (for debugging)? (proposed: yes, behind `--print-config`; secrets
   already redacted)

---

**RFC ends.**
