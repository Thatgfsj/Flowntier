# Release Plan

**Status (v0.4):** initial entry. Real release automation is
implemented as part of Phase 1 of the v0.4-delivery plan
(see PR #2).

## Cadence

* **Major** (x.0.0) — backward-incompatible change to the
  workflow or provider protocol. Requires an RFC.
* **Minor** (0.x.0) — new user-facing capability. Bug-fix + small
  improvements.
* **Patch** (0.0.x) — bug fixes only.

The maintainer (Thatgfsj) cuts releases on `main`; there is no
fixed calendar. Releases happen when a meaningful chunk of work is
ready.

## Process

1. Cut a release branch from `main`: `release/vX.Y.Z`.
2. Run the full acceptance suite (28 backend + 6 UI). All must pass.
3. Bump version in 4 places via `tools/bump_version.sh`:
   * `tauri.conf.json` `version`
   * `apps/desktop/src-tauri/Cargo.toml` `version`
   * `apps/desktop/package.json` `version`
   * `Cargo.toml` `[workspace.package] version`
4. Tag the commit: `git tag -s vX.Y.Z -m "Release vX.Y.Z"`.
5. Push the tag; CI produces the draft GitHub Release with
   NSIS + MSI + dmg + deb + AppImage installers.
6. Smoke-test on Windows (NSIS + MSI), macOS (dmg), Linux
   (AppImage + deb).
7. Promote the draft Release to public.

## Upgrader

The Tauri updater plugin auto-checks GitHub Releases for new
versions. Signature verification is mandatory (see
`docs/SECURITY.md`).

## Channels

Currently a single channel (latest). Beta / RC channels land in v1.0.

## Past releases

See `CHANGELOG.md` at the repository root for the full list.