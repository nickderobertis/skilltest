#!/usr/bin/env bash
# Pin every package in the workspace to a single lockstep version and refresh all
# lockfiles. This is the one place the repo's version is written.
#
# Invoked by semantic-release on each release (.releaserc.json -> exec.prepareCmd)
# and runnable by hand to set/realign the baseline. Idempotent: every edit *sets*
# the value (never increments), so re-running with the same version changes nothing
# and the lock refreshes no-op. Quiet on success; on failure prints the exact error
# and a suggested next action (repo convention; see AGENTS.md "Scripts and output").
#
#   scripts/set-version.sh 0.4.1
#
# Needs cargo, uv, and pnpm on PATH (it refreshes Cargo.lock, both uv.locks, and
# pnpm-lock.yaml). Touches: Cargo.toml (+ the internal skilltest-core pin), both
# pyproject.toml (+ pytest's exact skilltest-sdk pin), both package.json, the four
# @skill-test/cli-* platform package.json, and the four lockfiles. The SDK's and
# vitest's `workspace:*` deps and pytest's editable [tool.uv.sources] are left alone.
set -euo pipefail

VERSION="${1:-}"
if [ -z "$VERSION" ]; then
  echo "error: set-version.sh requires a semver version, e.g. set-version.sh 0.4.1" >&2
  exit 2
fi
# Accept X.Y.Z plus an optional prerelease/build suffix (semantic-release may pass one).
if ! printf '%s' "$VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)*$'; then
  echo "error: '$VERSION' is not a semver version (expected like 0.4.1)" >&2
  exit 2
fi

cd "$(dirname "$0")/.."

# Run a lockfile refresh, swallowing output on success and surfacing the full output
# plus a hint only when it fails.
run() { # <description> <command...>
  local desc="$1"; shift
  local out
  if ! out="$("$@" 2>&1)"; then
    {
      echo "error: $desc failed"
      echo "$out"
      echo "hint: run \`$*\` by hand to see the underlying failure."
    } >&2
    exit 1
  fi
}

uv_lock_in() { ( cd "$1" && uv lock --quiet ); }

# --- Rust: [workspace.package].version + the internal skilltest-core pin, then lock --
perl -i -pe 's/^version = "[^"]*"/version = "'"$VERSION"'"/' Cargo.toml
perl -i -pe 's{(skilltest-core = \{ path = "crates/skilltest-core", version = ")[^"]*(")}{${1}'"$VERSION"'${2}}' Cargo.toml
run "refresh Cargo.lock" cargo update --quiet -p skilltest-core -p skilltest-cli

# --- Python: both project versions + pytest's exact skilltest-sdk pin, then locks ----
perl -i -pe 's/^version = "[^"]*"/version = "'"$VERSION"'"/' sdks/python/pyproject.toml
perl -i -pe 's/^version = "[^"]*"/version = "'"$VERSION"'"/' plugins/pytest/pyproject.toml
perl -i -pe 's/"skilltest-sdk[^"]*"/"skilltest-sdk=='"$VERSION"'"/' plugins/pytest/pyproject.toml
run "refresh sdks/python/uv.lock" uv_lock_in sdks/python
run "refresh plugins/pytest/uv.lock" uv_lock_in plugins/pytest

# --- TypeScript: SDK + framework + the four optional platform packages, then lock ----
# The platform packages (@skill-test/cli-<os>-<arch>) carry the binary; the SDK
# pins them via `workspace:*`, so they must stay on the same version. The SDK's
# `workspace:*` optional deps and vitest's `workspace:*` dep are left alone (pnpm
# rewrites them to the version on publish).
perl -i -pe 's/"version": "[^"]*"/"version": "'"$VERSION"'"/' sdks/typescript/package.json
perl -i -pe 's/"version": "[^"]*"/"version": "'"$VERSION"'"/' plugins/vitest/package.json
for pkg in sdks/typescript/platforms/*/package.json; do
  perl -i -pe 's/"version": "[^"]*"/"version": "'"$VERSION"'"/' "$pkg"
done
run "refresh pnpm-lock.yaml" pnpm install --lockfile-only --reporter=silent

echo "set-version: all packages pinned to $VERSION"
