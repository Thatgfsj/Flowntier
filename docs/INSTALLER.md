# Installer Build

> **How to build the Flowntier desktop installer.**
>
> Last verified: 2026-06-25
> Maintainer: Thatgfsj

Flowntier ships three operating systems from the same Rust workspace:

| OS      | Targets                            | Bundle formats                          |
|---------|------------------------------------|-----------------------------------------|
| Windows | `x86_64-pc-windows-msvc`           | NSIS `.exe` + WiX `.msi`               |
| Linux   | `x86_64-unknown-linux-gnu`         | `.deb` + `.AppImage`                    |

macOS and Linux RPM are deferred to v0.5 — see ROADMAP.md.

The CI matrix builds both. See `.github/workflows/release.yml`.

---

## 1. What gets built (per OS)

| Artifact                                    | Path                                                                | Approx size |
|---------------------------------------------|---------------------------------------------------------------------|-------------|
| Windows NSIS setup                          | `target/release/bundle/nsis/Flowntier_<v>_x64-setup.exe`            | ~31 MB      |
| Windows WiX MSI                             | `target/release/bundle/msi/Flowntier_<v>_x64_en-US.msi`             | ~40 MB      |
| Windows standalone `.exe`                   | `target/release/flowntier-desktop.exe`                              | ~16 MB      |
| Linux `.deb`                                | `target/release/bundle/deb/Flowntier_<v>_amd64.deb`                 | ~50 MB      |
| Linux `.AppImage`                           | `target/release/bundle/appimage/Flowntier_<v>_amd64.AppImage`       | ~80 MB      |

The NSIS setup is the right format for **public distribution** — it puts a
"Flowntier" shortcut on the desktop, registers an uninstaller under
"Add or remove programs", and shows a license dialog.

The MSI is the right format for **corporate / Group Policy** deployment.

The standalone `.exe` is for development: it requires no install but
does not auto-update, register file associations, or appear in
"Apps & Features".

Sizes above are estimates from the v0.4.0 build. They will be
refreshed in Phase 6 after the final `pnpm tauri:build` smoke test.

---

## 2. Prerequisites for the build host

* **Rust** ≥ 1.85 (`rustup toolchain install stable`)
* **Node.js** ≥ 24 (`node --version`)
* **pnpm** ≥ 9 (`npm install -g pnpm`)
* **Windows only**: Visual Studio Build Tools 2022 with the
  "Desktop development with C++" workload (provides MSVC + linker)
* **Windows only**: WiX Toolset v3 (downloaded automatically by
  the Tauri 2 bundler; no manual install needed)
* **Windows only**: NSIS (downloaded automatically by the Tauri
  2 bundler; no manual install needed)
* **Linux build host** (Ubuntu 22.04+ recommended):
  `apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev patchelf build-essential libssl-dev`

The first release build is slow (~10 minutes) because the release
profile compiles `tao`, `wry`, and `tauri` from scratch. Incremental
rebuilds are fast.

---

## 3. Build steps (local smoke test)

```bash
# 1. One time, from the repo root
pnpm install
cargo build --release -p pipe-server --bin flowntier-runtime \
            --target x86_64-pc-windows-msvc

# 2. Stage the Rust sidecar binary where Tauri 2 expects it.
#    Tauri 2 wants: <basename>-<target-triple><ext>
mkdir -p apps/desktop/src-tauri/binaries
cp target/x86_64-pc-windows-msvc/release/flowntier-runtime.exe \
   apps/desktop/src-tauri/binaries/flowntier_runtime-x86_64-pc-windows-msvc.exe

# 3. Build the installer (frontend + Rust shell + bundles)
cd apps/desktop
pnpm tauri:build
```

Output goes to `apps/desktop/target/release/bundle/{nsis,msi,deb,appimage}/`.

### Faster alternatives for iteration

```bash
# Skip the bundler — just verify the config + frontend + shell compile
pnpm tauri build --no-bundle

# Only one bundle format (faster)
pnpm tauri build --bundles nsis
pnpm tauri build --bundles msi
```

---

## 4. The CI build

Push a tag of the form `v*`:

```bash
git tag -s v0.4.0-rc1 -m 'Release v0.4.0-rc1'
git push origin v0.4.0-rc1
```

`.github/workflows/release.yml` then:

1. `build-runtime` — compiles `flowntier-runtime` for each of the
   3 targets in parallel, uploads as `sidecar-<target>` artifact.
2. `build-desktop` — for each OS runner, downloads the matching
   sidecar artifact, installs Linux deps if needed, runs
   `tauri-apps/tauri-action@v0` to produce installers + a
   **draft** GitHub Release.
3. `publish-release` — promotes the draft Release to public and
   attaches the updater's `latest-<target>-<arch>.json` + `.sig`
   files so the in-app auto-updater works.

After the CI run, verify the Release by:

* Downloading the artifact that matches your OS.
* Installing it on a clean VM.
* Launching — the React UI should render within ~3 seconds.
* Triggering an update from an older v0.4.x → newer v0.4.x to
  exercise the updater signature verification.

---

## 5. Configuration: `apps/desktop/src-tauri/tauri.conf.json`

The interesting fields:

```jsonc
{
  "productName": "Flowntier",
  "version": "0.4.0",            // bumped via tools/bump_version.sh
  "identifier": "ai.flowntier.desktop",
  "bundle": {
    "active": true,
    "targets": "all",
    "publisher": "Thatgfsj",
    "copyright": "Copyright © 2026 Thatgfsj",
    "homepage": "https://github.com/Thatgfsj/Flowntier",
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.ico"
    ],
    "externalBin": ["binaries/flowntier_runtime"],
    "resources": ["WebView2Loader.dll"],
    "windows": {
      "nsis": {
        "installerIcon": "icons/icon.ico",
        "installMode": "perMachine",
        "languages": ["SimpChinese", "English"],
        "displayLanguageSelector": true
      },
      "wix": { "language": ["en-US"] }
    },
    "linux": {
      "deb": {
        "depends": [
          "libwebkit2gtk-4.1-0",
          "libgtk-3-0",
          "libayatana-appindicator3-1",
          "libnss3", "libxss1", "libasound2"
        ]
      }
    }
  },
  "plugins": {
    "updater": {
      "active": true,
      "dialog": true,
      "endpoints": [
        "https://github.com/Thatgfsj/Flowntier/releases/latest/download/{{target}}-{{arch}}.json"
      ],
      "pubkey": "<152-char base64 ed25519 public key>",
      "windows": { "installMode": "passive" }
    }
  }
}
```

Notable choices:

* **`identifier`** must NOT end in `.app` (Tauri's own warning).
  We use `ai.flowntier.desktop`.
* **`externalBin`** lists sidecar binaries that ship next to the
  app exe. Currently just `flowntier_runtime`. Tauri 2 expects
  `<basename>-<target-triple><ext>` next to the entry; see
  §3 step 2.
* **`resources`** lists files that must be present at runtime
  even if the app does not directly embed them.
  `WebView2Loader.dll` is the Edge WebView2 bootstrapper for
  Windows installs that don't have Edge pre-installed.
* **`bundle.windows.nsis.languages`** picks which installers
  NSIS downloads. `displayLanguageSelector: true` lets the user
  pick at install time.
* **`bundle.linux.deb.depends`** declares runtime libraries the
  `.deb` requires. Lintian will warn if any are missing on a
  fresh Ubuntu 22.04 install.
* **`plugins.updater.pubkey`** is the ed25519 public key
  compiled into the shell. The matching private key is held in
  the `TAURI_SIGNING_PRIVATE_KEY` GitHub secret — see §6.

---

## 6. Auto-update (signing key setup)

The v0.4 release uses Tauri's `tauri-plugin-updater` with
signature verification enabled. Signatures are verified before
any update is applied — without a valid signature, the update
is refused.

### One-time key generation

Run **once** on a secure machine (the key holder's workstation,
not CI):

```bash
cd apps/desktop
pnpm tauri signer generate --ci -w .keys/flowntier-signing.key
```

You'll see output like:

```
Your keypair was generated successfully:
Private: .keys/flowntier-signing.key   (Keep it secret!)
Public:  .keys/flowntier-signing.key.pub
```

* `flowntier-signing.key` is the **private key**. Add it to
  GitHub repo secrets **once** and never store it anywhere else:

  ```
  github.com → Thatgfsj/Flowntier →
    Settings → Secrets and variables → Actions →
    "New repository secret"
      Name : TAURI_SIGNING_PRIVATE_KEY
      Value: <full contents of flowntier-signing.key>
  ```

  (Optional but recommended) Add a second secret for the
  password:

  ```
      Name : TAURI_SIGNING_PRIVATE_KEY_PASSWORD
      Value: <the password you chose at generation time>
  ```

* `flowntier-signing.key.pub` is the **public key**. It is
  already committed to `tauri.conf.json` (`plugins.updater.pubkey`).
  When you rotate keys, **commit the new pubkey** in the same
  PR that bumps the version. Old clients continue to verify
  against the old pubkey compiled into their build.

### Verify it's wired

After pushing the secrets, trigger a release (push a `v*` tag).
The CI logs should include:

```
GITHUB_TOKEN                  *** set
TAURI_SIGNING_PRIVATE_KEY     *** set
TAURI_SIGNING_PRIVATE_KEY_PASSWORD *** set (or empty)
```

And the published Release should have these artifacts (per
target):

* `latest-<target>-<arch>.json`
* `latest-<target>-<arch>.json.sig`

### Rotation

If the key is lost or compromised:

1. Generate a new keypair (`tauri signer generate -w .keys/flowntier-signing.key -f`).
2. Update the `TAURI_SIGNING_PRIVATE_KEY` GitHub secret.
3. Commit the new pubkey to `tauri.conf.json` in a new PR.
4. Cut a release with the new key (`vX.Y.Z+1`).
5. **All users will need to manually download the new release
   and reinstall** — old versions will refuse to update because
   the signatures were made with the old key.

This is why we keep the key on a single maintainer's workstation,
not in CI.

---

## 7. Code signing (deferred to v0.5)

In v0.4 the installer is **unsigned**:

* Windows SmartScreen will show "Unknown publisher" on first
  launch. Users click **More info → Run anyway**.
* macOS Gatekeeper will block the `.dmg` on first launch. Users
  right-click → Open. (Not in v0.4; macOS deferred to v0.5.)
* Linux `.deb` will install without warning; `.AppImage` will
  ask for execute permission.

Adding code signing is a v0.5 follow-up. For Windows, an EV cert
from DigiCert / Sectigo (~$300–500/yr, stored on a hardware token)
is required for SmartScreen reputation. macOS notarisation
  (an Apple Developer ID cert + `xcrun notarytool`) lands with
  the macOS support in v0.5. See `docs/SECURITY.md` for the
threat model and why we deferred this.

---

## 8. Verifying the installer locally

```powershell
# NSIS setup: silent install + uninstall
'.\target\release\bundle\nsis\Flowntier_0.4.0_x64-setup.exe' /S
& 'C:\Program Files\Flowntier\flowntier-desktop.exe'
'C:\Program Files\Flowntier\uninstall.exe' /S   # silent uninstall

# MSI: silent install + uninstall (run in a VM!)
msiexec /i 'target\release\bundle\msi\Flowntier_0.4.0_x64_en-US.msi' /qn
& 'C:\Program Files\Flowntier\flowntier-desktop.exe'
msiexec /x 'target\release\bundle\msi\*.msi' /qn
```

The app data directory is at `%APPDATA%\flowntier\`. If you need
a clean test, delete it before launching:

```powershell
Remove-Item -Recurse -Force $env:APPDATA\flowntier
```

Logs land at `%APPDATA%\flowntier\logs\flowntier.log.YYYY-MM-DD`
(added in Phase 2 of v0.4-delivery).

---

## 9. What changed in v0.4

* **Removed**: Python sidecar build (apps/runtime/ + PyInstaller).
  Replaced by `crates/pipe-server` Rust sidecar.
* **Removed**: `acme.sh` certbot step from CI (no HTTPS yet).
* **Added**: `bundle.windows.nsis` so the installer produces
  a real uninstaller.
* **Added**: `bundle.macOS` for `.dmg` packaging. (Phase 1
  shipped this; Phase 0.5 removed it because the chairman
  scoped v0.4 to Windows + Linux only. macOS resumes in v0.5.)
* **Added**: `bundle.linux.deb.depends` so `.deb` declares its
  runtime libraries.
* **Added**: `tauri-plugin-updater` with GitHub Releases endpoint.
* **Added**: signing key generation workflow (this doc §6).
* **Changed**: `targets: "all"` produces all bundle formats in
  one CI run instead of needing multiple jobs.

Historical installer notes (v0.1 through v0.3) are in
`history/docs/` for paper references.