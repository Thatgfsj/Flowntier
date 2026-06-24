# Installer Build

> **How to build the Flowntier desktop installer for Windows.**
>
> Last verified: 2026-06-24
> Maintainer: Thatgfsj

---

## What gets built

`pnpm tauri:build` (run from `apps/desktop/`) produces three
artifacts in `target/release/`:

| Artifact | Path | Size (Windows, x64) |
|----------|------|---------------------|
| Standalone `.exe` | `target/release/flowntier-desktop.exe` | ~16 MB |
| MSI installer | `target/release/bundle/msi/Flowntier_0.2.5_x64_en-US.msi` | ~40 MB |
| NSIS setup | `target/release/bundle/nsis/Flowntier_0.2.5_x64-setup.exe` | ~31 MB |

The MSI is the right format for **corporate / Group Policy**
deployment. The NSIS setup is the right format for **public
distribution** — it puts a "Flowntier" shortcut on the desktop,
registers an uninstaller under "Add or remove programs", and
shows a licence dialog (currently the default Tauri one).

The standalone `.exe` is for development: it requires no
install but does not auto-update, register file associations,
or appear in "Apps & Features".

---

## Prerequisites for the build host

* **Rust** ≥ 1.85 (`rustup toolchain install stable`)
* **Node.js** ≥ 24 (`node --version`)
* **pnpm** ≥ 9 (`npm install -g pnpm`)
* **Tauri CLI** (`cargo install tauri-cli@^2 --locked`)
* **Windows only**: WiX Toolset v3 (`cargo install cargo-wix`
  is enough; the tool will fetch the binary automatically)
* **Windows only**: NSIS (downloaded automatically by the Tauri
  tool)

The build itself is slow on first run (~8 minutes) because the
release profile compiles `tao`, `wry`, and `tauri` from
scratch. Incremental rebuilds are fast.

---

## Build steps

```bash
# One time, from the repo root
pnpm install
cargo build --release -p pipe-server

# Stage the Rust sidecar binary into the bundle resources
mkdir -p apps/desktop/src-tauri/binaries
cp target/release/flowntier-runtime.exe \
   apps/desktop/src-tauri/binaries/aco_runtime-x86_64-pc-windows-msvc.exe

# Build the installer
cd apps/desktop
pnpm tauri:build
```

The output goes to `apps/desktop/target/release/bundle/{msi,nsis}/`.
Apps/desktop's `target/` is git-ignored.

---

## Verifying the installer

```powershell
# 1. Inspect the MSI metadata
Get-Item 'target\release\bundle\msi\*.msi' | Format-List Name,Length,LastWriteTime

# 2. Try a silent install + uninstall (in a VM!)
msiexec /i 'target\release\bundle\msi\Flowntier_0.2.5_x64_en-US.msi' /qn
# Launch the app
& 'C:\Program Files\Flowntier\flowntier-desktop.exe'
# Uninstall
msiexec /x 'target\release\bundle\msi\*.msi' /qn
```

For the NSIS setup:

```powershell
'.\target\release\bundle\nsis\Flowntier_0.2.5_x64-setup.exe' /S   # silent install
& 'C:\Program Files\Flowntier\flowntier-desktop.exe'
'C:\Program Files\Flowntier\uninstall.exe' /S                       # silent uninstall
```

---

## Configuration: `apps/desktop/src-tauri/tauri.conf.json`

The interesting fields:

```json
{
  "productName": "Flowntier",
  "version": "0.2.5",
  "identifier": "ai.flowntier.desktop",
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": ["icons/icon.ico"],
    "publisher": "Thatgfsj",
    "category": "DeveloperTool",
    "externalBin": ["binaries/aco_runtime"],
    "resources": ["WebView2Loader.dll"]
  }
}
```

* `identifier` must NOT end in `.app` (Tauri's own warning).
  We use `ai.flowntier.desktop`.
* `externalBin` lists sidecar binaries that ship next to the
  app exe. Currently just `aco_runtime` (the JSON-RPC + event
  pipe server). The Tauri shell can either `Command::new`
  this binary on startup or be linked to it in-process
  (the v0.3 plan).
* `resources` lists files that must be present at runtime
  even if the app does not directly embed them. `WebView2Loader.dll`
  is the Edge WebView2 bootstrapper for Windows installs that
  don't have Edge pre-installed.

---

## Code signing

**Currently unsigned.** That means:
* Windows SmartScreen will show "Unknown publisher" on first
  launch.
* macOS Gatekeeper will block the .dmg.
* Most package managers will refuse to install.

Before publishing, set up:
* Windows: an EV cert from DigiCert / Sectigo (~$300-500/yr),
  stored in a hardware token. Configure `bundle.windows.signingIdentity`
  in `tauri.conf.json`.
* macOS: an Apple Developer ID cert + notarisation. Configure
  `bundle.macOS.signingIdentity` and run `xcrun notarytool` as
  a CI step.

This is documented in `tauri.conf.json` only as a placeholder
and is out of scope for v0.4.

---

## Auto-update

**Currently disabled.** Tauri has `tauri-plugin-updater` which
checks a JSON manifest URL on launch and prompts the user to
update. Enabling it requires:

1. A static URL hosting `latest.json` with version + asset
   metadata.
2. A signing key pair; the public key goes in `tauri.conf.json`.
3. The CI to publish `latest.json` + the bundle artifacts to
   that URL on every tagged release.

We have not enabled this in v0.4 because:
* We are pre-release (v0.2.5).
* The pre-release build is not stable enough that users want
  automatic silent upgrades.
* A misconfigured updater is a great way to brick every user's
  install at once.

`tauri-plugin-updater` is in the dependency tree already; flip
it on when we hit v1.0.

---

## What this run actually produced

Run on 2026-06-24 (commit immediately after `pnpm tauri:build`):

```
✓ built in 9.51s    (Vite frontend build)
✓ 8m 06s             (Rust release compile, tauri + tao + wry + plugins)
✓ MSI    Flowntier_0.2.5_x64_en-US.msi         40,726,528 bytes (38.8 MB)
✓ NSIS   Flowntier_0.2.5_x64-setup.exe         31,228,408 bytes (29.8 MB)
✓ exe    target/release/flowntier-desktop.exe              ~16 MB
```

Smoke-tested by spawning `flowntier-desktop.exe` from a fresh shell:
the window opened, the React UI rendered, and the
`run_agent_task` Tauri command registered correctly.

The two installer warnings (`*.app` identifier, dynamic vs
static import) are fixed in commit `78a4d77`.
