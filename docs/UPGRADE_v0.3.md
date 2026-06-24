# Upgrade guide: v0.2.5 → v0.3.0 (Flowntier rename)

> **TL;DR.** Uninstall v0.2.5, install v0.3.0. Your existing
> data directory (`~/.config/aco/` + `~/.local/share/aco/`,
> or `%APPDATA%/aco/` on Windows) is auto-migrated on first
> launch to the equivalent `flowntier/` paths. No user
> action is required beyond the uninstall-then-install.

This release is **a brand rename only.** No new features, no
schema rewrites, no behaviour changes. The product is now
called **Flowntier** ("Flow" + "Frontier") instead of
**Agent Company OS** (ACO).

---

## What changed under the hood

### 1. Tauri bundle identifier

```
dev.acos.desktop  →  ai.flowntier.desktop
```

The bundle identifier is the per-platform identity Tauri
uses for the installed app (registry keys on Windows,
`Info.plist` on macOS, `.desktop` file on Linux).
Windows installers will not auto-upgrade over an app with a
different identifier, so v0.3.0 will land as a **fresh
install alongside v0.2.5**, not an in-place upgrade.

### 2. npm scope

Every workspace package renamed from `@aco/*` to `@flowntier/*`:

```
@aco/shared       →  @flowntier/shared
@aco/ui           →  @flowntier/ui
@aco/workflow     →  @flowntier/workflow
@aco/providers    →  @flowntier/providers
@aco/prompts      →  @flowntier/prompts
@aco/desktop      →  @flowntier/desktop
```

If you maintain a plugin or extension that imports from the
workspace packages, run `pnpm install` after pulling and let
the rename flow through your lockfile.

### 3. Runtime data directory

The Rust core's default paths moved from `aco/` to `flowntier/`.
On first launch of v0.3.0, the new code detects any legacy
`aco/` directories at the OS-default locations and renames
them in place:

| OS | Legacy | New |
|---|---|---|
| Windows | `%APPDATA%\aco\` | `%APPDATA%\flowntier\` |
| Linux | `~/.config/aco/` and `~/.local/share/aco/` | `~/.config/flowntier/` and `~/.local/share/flowntier/` |
| macOS | `~/Library/Application Support/aco/` | `~/Library/Application Support/flowntier/` |

A one-line message is written to stderr:

```
[flowntier] migrated data dir: <old> -> <new>
[flowntier] migrated config dir: <old> -> <new>
```

If the rename fails (file lock, permissions, cross-device
link), v0.3.0 falls back to a fresh `flowntier/` directory
and leaves the legacy `aco/` directory in place. Your old
state will not be lost, but you'll need to copy it manually
if you want it back.

### 4. SQLite schema

New migration `0002_rename_aco_to_flowntier.sql`:

```sql
ALTER TABLE config_snapshots RENAME COLUMN aco_toml TO flowntier_toml;
```

This runs once when an existing v0.2.x DB is opened by
v0.3.0. New DBs (fresh installs) skip it because the column
is already named `flowntier_toml`.

### 5. Windows named pipes / Unix sockets

| v0.2.5 | v0.3.0 |
|---|---|
| `\\.\pipe\aco_runtime` | `\\.\pipe\flowntier_runtime` |
| `\\.\pipe\aco_runtime_events` | `\\.\pipe\flowntier_runtime_events` |
| `~/.cache/aco/sockets/aco_runtime.sock` | `~/.cache/flowntier/sockets/flowntier_runtime.sock` |
| `~/.cache/aco/sockets/aco_runtime_events.sock` | `~/.cache/flowntier/sockets/flowntier_runtime_events.sock` |

Unix socket files are deleted before bind (existing logic
in `crates/pipe-server/src/server.rs`), so a stale socket
left over from a crashed v0.2.5 process does not block
v0.3.0 startup.

### 6. Environment variable prefix

The coding-rules prefix moved from `ACO_` to `FLOWNTIER_`:

```
ACO_LOG_LEVEL       →  FLOWNTIER_LOG_LEVEL
ACO_DATA_DIR        →  FLOWNTIER_DATA_DIR
ACO_THEME           →  FLOWNTIER_THEME
ACO_DISABLE_PLUGIN_X → FLOWNTIER_DISABLE_PLUGIN_X
```

These overrides are **not yet honored by the runtime** in
v0.3.0 — the rename only updated the rule docs. If you
were relying on `ACO_*` overrides from v0.2.x, they are
silently ignored until v0.4 lands the env override layer.

---

## Step-by-step upgrade (Windows)

```powershell
# 1. Stop and uninstall v0.2.5.
taskkill /F /IM flowntier-desktop.exe 2>$null
& 'C:\Program Files\Agent Company OS\uninstall.exe' /S

# 2. (Optional) confirm the legacy data dir is still there
#    — v0.3.0 will rename it in place on first launch.
dir $env:APPDATA\aco

# 3. Install v0.3.0 (NSIS setup or MSI).
.\Flowntier_0.3.0_x64-setup.exe

# 4. Launch.
& 'C:\Program Files\Flowntier\flowntier-desktop.exe'

# 5. Verify the migration. You should see two lines on stderr
#    (only visible if you launched from a terminal, not from
#    the Start Menu):
#
#    [flowntier] migrated config dir: C:\Users\you\AppData\Roaming\aco -> C:\Users\you\AppData\Roaming\flowntier
#    [flowntier] migrated data dir:   C:\Users\you\AppData\Roaming\aco -> C:\Users\you\AppData\Roaming\flowntier

# 6. Confirm the legacy dir is gone.
dir $env:APPDATA\aco   # should error: "File Not Found"
```

## Step-by-step upgrade (Linux / macOS)

```bash
# 1. Stop any running v0.2.5 instance.
pkill -f flowntier-runtime   # if you already had v0.3.0 prerelease running
pkill -f aco-runtime         # v0.2.5

# 2. (Optional) confirm legacy dirs are still there.
ls ~/.config/aco ~/.local/share/aco

# 3. Install v0.3.0 via your platform's package, or
#    run the standalone binary directly.

# 4. Launch. Watch stderr for the migration lines:
#    [flowntier] migrated config dir: /home/you/.config/aco -> /home/you/.config/flowntier
#    [flowntier] migrated data dir:   /home/you/.local/share/aco -> /home/you/.local/share/flowntier
```

---

## Rolling back to v0.2.5

If v0.3.0 misbehaves for some reason:

1. Uninstall v0.3.0.
2. The data-dir migration was a rename, not a copy — your
   legacy `aco/` dirs are gone. To restore v0.2.5 you'll
   need a backup of the `aco/` dirs from before the upgrade.
3. Install v0.2.5.
4. Restore `~/.config/aco/` and `~/.local/share/aco/` (or
   `%APPDATA%\aco\`) from backup.

**Tip:** before upgrading, make a one-time backup:

```bash
# Linux/macOS
tar czf aco-backup-$(date +%F).tar.gz \
    ~/.config/aco ~/.local/share/aco

# Windows (PowerShell)
$ts = Get-Date -Format 'yyyy-MM-dd'
Compress-Archive -Path "$env:APPDATA\aco" -DestinationPath "aco-backup-$ts.zip"
```

---

## What did NOT change

- Plugin manifest schema, plugin signing, plugin runtime
  ABI — all unchanged. Plugins compiled against the v0.2.5
  runtime API continue to work.
- The 8-phase workflow state machine, the SQLite schema
  beyond the `aco_toml → flowntier_toml` column rename,
  the JSON-RPC protocol shapes, the `WfEvent` event types.
- The 6 role prompts' structure (Identity / Responsibility /
  Out-of-scope / Workflow / Output format / Tools). Only
  the brand mention in the opening sentence changed.
- The GitHub repository URL (still
  `https://github.com/Thatgfsj/Flowntier` until you
  decide to rename the repo on the GitHub side).

---

## Questions / issues

File at `https://github.com/Thatgfsj/Flowntier/issues`
with the label `v0.3-upgrade`.