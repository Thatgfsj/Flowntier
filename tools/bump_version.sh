#!/usr/bin/env bash
#
# bump_version.sh — atomic version bump across all 4 manifests.
#
# Usage:
#   tools/bump_version.sh <new_version>
#
# Example:
#   tools/bump_version.sh 0.4.1
#
# Updates:
#   1. tauri.conf.json                         ("version": "X.Y.Z")
#   2. apps/desktop/src-tauri/Cargo.toml       (version = "X.Y.Z")
#   3. apps/desktop/package.json               ("version": "X.Y.Z")
#   4. Cargo.toml [workspace.package]          (version = "X.Y.Z")
#
# Validates:
#   * exactly one argument
#   * argument matches semver regex
#   * all 4 files exist
#   * all 4 files currently match (no in-progress bump)
#
# After running:
#   * verify with `git diff --stat`
#   * run `cargo check` + `pnpm typecheck`
#   * commit + tag

set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "usage: $0 <new-version>" >&2
    echo "  e.g. $0 0.4.1" >&2
    exit 2
fi

NEW="$1"

# Strict semver (no leading 'v', no pre-release for now)
if [[ ! "$NEW" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "error: '$NEW' is not a valid X.Y.Z semver (no 'v' prefix, no pre-release)" >&2
    exit 2
fi

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

TAURI_CONF="apps/desktop/src-tauri/tauri.conf.json"
DESKTOP_CARGO="apps/desktop/src-tauri/Cargo.toml"
DESKTOP_PKG="apps/desktop/package.json"
WORKSPACE_CARGO="Cargo.toml"

# Verify all files exist
for f in "$TAURI_CONF" "$DESKTOP_CARGO" "$DESKTOP_PKG" "$WORKSPACE_CARGO"; do
    if [[ ! -f "$f" ]]; then
        echo "error: $f does not exist (run from repo root?)" >&2
        exit 1
    fi
done

# Detect current version
CURRENT=$(grep -E '^version[[:space:]]*=' "$WORKSPACE_CARGO" \
    | head -1 | sed -E 's/^version[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/')

if [[ -z "$CURRENT" ]]; then
    echo "error: couldn't read current version from $WORKSPACE_CARGO" >&2
    exit 1
fi

if [[ "$CURRENT" == "$NEW" ]]; then
    echo "error: $WORKSPACE_CARGO already at $NEW — nothing to bump" >&2
    exit 1
fi

# Verify all 4 currently agree on $CURRENT
check_agrees() {
    local file="$1" pattern="$2" where="$3"
    local found
    found=$(grep -E "$pattern" "$file" | head -1 \
        | sed -E "s/$pattern/\1/")
    if [[ "$found" != "$CURRENT" ]]; then
        echo "error: $where reads '$found', expected '$CURRENT'" >&2
        echo "  fix the version drift manually before running this script" >&2
        exit 1
    fi
}

check_agrees "$WORKSPACE_CARGO" \
    '^version[[:space:]]*=[[:space:]]*"([^"]+)"' \
    "$WORKSPACE_CARGO [workspace.package]"
check_agrees "$DESKTOP_CARGO" \
    '^version[[:space:]]*=[[:space:]]*"([^"]+)"' \
    "$DESKTOP_CARGO [package]"
check_agrees "$DESKTOP_PKG" \
    '"version":[[:space:]]*"([^"]+)"' \
    "$DESKTOP_PKG"
check_agrees "$TAURI_CONF" \
    '"version":[[:space:]]*"([^"]+)"' \
    "$TAURI_CONF"

echo "Bumping version: $CURRENT → $NEW"

# 1. tauri.conf.json
sed -i.bak -E "s/\"version\":[[:space:]]*\"$CURRENT\"/\"version\": \"$NEW\"/" "$TAURI_CONF"
rm -f "$TAURI_CONF.bak"

# 2. apps/desktop/src-tauri/Cargo.toml (only the first match — the [package] one)
# Use awk for in-place editing since sed -i is non-portable.
awk -v cur="$CURRENT" -v new="$NEW" '
    /^version[[:space:]]*=/ && !done {
        gsub("\"" cur "\"", "\"" new "\"")
        done = 1
    }
    { print }
' "$DESKTOP_CARGO" > "$DESKTOP_CARGO.tmp" \
    && mv "$DESKTOP_CARGO.tmp" "$DESKTOP_CARGO"

# 3. apps/desktop/package.json
sed -i.bak -E "s/\"version\":[[:space:]]*\"$CURRENT\"/\"version\": \"$NEW\"/" "$DESKTOP_PKG"
rm -f "$DESKTOP_PKG.bak"

# 4. Cargo.toml [workspace.package]
sed -i.bak -E "s/^version[[:space:]]*=[[:space:]]*\"$CURRENT\"/version     = \"$NEW\"/" "$WORKSPACE_CARGO"
rm -f "$WORKSPACE_CARGO.bak"

echo ""
echo "Done. Verify with:"
echo "  git diff --stat"
echo "  grep -E '\"version\"|^version' $TAURI_CONF $DESKTOP_CARGO $DESKTOP_PKG $WORKSPACE_CARGO"
echo ""
echo "Then:"
echo "  cargo check --workspace"
echo "  pnpm typecheck"
echo "  git add -A && git commit -m 'chore(release): bump version to $NEW'"
echo "  git tag -s v$NEW -m 'Release v$NEW'"