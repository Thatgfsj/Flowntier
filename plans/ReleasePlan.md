# Release Plan

> Top-level release plan: how versions roll up, what each version
> ships to users, how we communicate.
>
> **Status (v0.4):** rewritten to match the v0.4-delivery plan.
> The original v0.1 phase → version mapping is archived in
> `history/plans/ReleasePlan.md`.

**Version:** v0.4
**Last updated:** 2026-06-24

---

## 1. Cadence

* **Major** (x.0.0) — backward-incompatible change to the
  workflow or provider protocol. Requires an RFC.
* **Minor** (0.x.0) — new user-facing capability. Bug fixes +
  small improvements.
* **Patch** (0.0.x) — bug fixes only.

The maintainer (Thatgfsj) cuts releases on `main`; there is no
fixed calendar. Releases happen when a meaningful chunk of work
is ready.

## 2. Process

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

## 3. Channels

Currently a single channel (latest). Beta / RC channels land
in v1.0.

## 4. v0.4-delivery phases

| Phase | What ships                                                |
|-------|-----------------------------------------------------------|
| 0     | Brand rename, source-of-truth version, doc stubs          |
| 1     | Release CI rewrite + Tauri updater + NSIS/WiX bundle      |
| 2     | Frontend safety net: ErrorBoundary + i18n + logging + CSP |
| 3     | Persistent secrets (DPAPI/Keychain/libsecret) + real provider endpoints |
| 4     | First-run Welcome screen + sample workflow                |
| 5     | Version handshake between shell and sidecar               |
| 6     | README + INSTALLER + SECURITY + FAQ polish, screenshots, tag v0.4.0 |

Phases ship as separate PRs. A phase is "done" only when its
exit criterion (in the v0.4-delivery plan) is met and CI is green.

## 5. Upgrader

The Tauri updater plugin auto-checks GitHub Releases for new
versions. Signature verification is mandatory (see
`docs/SECURITY.md`).

## 6. Past releases

See `CHANGELOG.md` at the repository root for the full list.
Pre-v0.4 release notes are in `history/release-notes/`.