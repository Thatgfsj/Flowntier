# Deprecations

This document tracks APIs, file paths, and configuration keys that
have been deprecated but kept for backward compatibility.

**Status (v0.4):** initial entry.

## Internal Rust identifiers

These are deprecated and slated for removal in v0.5 or v1.0. They
are still present in the codebase because removing them touches
the SQLite migration path and test fixtures; the rename will land
in a dedicated PR to keep blast radius small.

| Old name             | Replacement                  | Plan                          |
|----------------------|------------------------------|-------------------------------|
| `struct AcoConfig`   | `struct FlowntierConfig`     | v1.0                          |
| `fn load_aco_config` | `fn load_flowntier_config`   | v1.0                          |
| `config/aco.toml`    | `config/flowntier.toml`      | already renamed in v0.4.0     |
| SQL column `aco_toml`| `flowntier_toml`             | renamed by migration 0002     |

## User-facing deprecations

None yet. v0.4 is the first release that aims at real users; any
backward-incompatible change made *after* v0.4 ships will be
recorded here with a removal version.

## Process

When deprecating something:

1. Mark the symbol with `#[deprecated(since = "X.Y", note = "...")]`
   (Rust) or `@deprecated` (TS).
2. Add a row to this table.
3. Keep the deprecated symbol working until the removal version.
4. Remove in a dedicated PR that also updates this table.