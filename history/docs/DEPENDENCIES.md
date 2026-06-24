# Dependencies

> Supply-chain notes for Agent Company OS.
> Last verified: 2026-06-23.

## Audit tooling

We use [`cargo-deny`](https://embarkstudios.github.io/cargo-deny/)
configured by `deny.toml` at the repo root. Run it with:

```bash
cargo deny check
```

Configuration is intentionally **advisory-only** during pre-release:

- `wildcards = "allow"` — Cargo workspace path deps are reported
  as "wildcard" by cargo-deny because their version is implicit
  (= workspace version). This is the correct way to express
  in-tree deps and we don't want to fight cargo-deny on it.
- `licenses` is permissive enough for Tauri's GTK transitives.
- `advisories` flagged a list of crates; see below.

Tighten the configuration to `deny` before the first published
release.

## Advisory scan (`cargo deny check advisories`)

The current `cargo deny check` reports 15 advisories. **All of
them are `unmaintained` warnings (no real CVEs / RUSTSEC-level
vulnerabilities).** They fall into two clusters:

### Cluster A — WebKitGTK bindings (Linux-only)

Tauri's WebView on Linux is backed by `webkit2gtk`, which
transitively pulls in GTK 3 Rust bindings. None of these are
vulnerable in a security sense — they are flagged only because
upstream is slow to respond to issues:

| Crate | Advisory |
|-------|----------|
| `atk`, `atk-sys` | RUSTSEC-2024-0413 / 0416 |
| `gdk`, `gdk-sys`, `gdkwayland-sys`, `gdkx11`, `gdkx11-sys` | RUSTSEC-2024-0412 / 0414 / 0415 |
| `gtk`, `gtk-sys`, `gtk3-macros` | RUSTSEC-2024-0417 / 0418 / 0419 |
| `proc-macro-error` | RUSTSEC-2024-0420 |

**Linux impact only.** macOS and Windows users do not pull
these in. The risk is "the GTK Rust bindings may accumulate
unfixed issues over time" rather than "your machine is
exploitable today".

**Mitigation strategy:** when Linux support becomes a hard
requirement (post-v1.0), evaluate switching the WebView
backend to one of:

- `tao` + `softbuffer` (no GTK at all, much smaller surface)
- `webkit2gtk4` (GTK 4 binding, maintained upstream)
- Stay on `webkit2gtk` and pin past the unmaintained version
  (current state)

### Cluster B — `unic-*` family

`unic-char-property`, `unic-char-range`, `unic-common`,
`unic-ucd-ident`, `unic-ucd-version` (all 0.9.0) are flagged
as `unmaintained`. They are pulled in transitively by
`idna_adapter` via `url`. No CVE.

**Mitigation:** most are pure-data crates whose UCD tables
rarely change; they're fine to keep until we exercise a URL
parser edge case. RUSTSEC-2017-0008 is the historic IDNA-UTS46
discrepancy, which `idna_adapter` does not affect us (we don't
parse user-supplied URLs into hostnames).

### `serial` 0.4.0

Crates.io yanked, pulled in by `tauri-build`. Tracked at
RUSTSEC-2025-0075. Tauri's build script only uses `serial`
for log output to stdout during codegen; no runtime impact.

## License scan (`cargo deny check licenses`)

Permit list (see `deny.toml`):

```
MIT, Apache-2.0 (+ LLVM-exception), BSD-2-Clause, BSD-3-Clause,
ISC, Unicode-DFS-2016, Unicode-3.0, CC0-1.0, Zlib, OpenSSL,
MPL-2.0, MIT-0
```

Tauri pulls in `MPL-2.0` (Mozilla Public License) transitively.
We have not audited every transitive crate, so `licenses =
"warn"` until first published release.

## Duplicate versions

`cargo deny check bans` flags ~15 crates pulled in at multiple
versions (e.g. `bitflags 1.x` from `clap` legacy code + `bitflags
2.x` from `serde`). All are intentional upstream choices; we
do not have a `cargo update --aggressive` story because the
resulting breakage is not worth the cosmetic improvement.

## What we should do before publishing

- [ ] Tighten `wildcards = "deny"` and replace path deps with
      versioned workspace deps where appropriate.
- [ ] Tighten `licenses = "deny"` after explicit GPL/AGPL
      review (we are not aware of any in the tree).
- [ ] Switch off `webkit2gtk` GTK-3 path or pin past the
      unmaintained advisory.
- [ ] Re-run `cargo audit` (separate tool) for CVE-level
      issues.
- [ ] Set up `cargo-deny` as a required CI gate.

---

**Bottom line:** there are no known exploitable security issues
in the runtime today. The `cargo-deny` noise is dominated by
Tauri's GTK/IDNA transitive dependencies and is documented
for transparency, not as a call to action.
