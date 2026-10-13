#!/usr/bin/env bash
# Bundle smoke (npm): prove a published-shape `@skill-test/sdk` runs the CLI
# **bundled in its platform package**. Pack the SDK, the matching
# `@skill-test/cli-*` package (binary staged), and the vitest plugin; install the
# tarballs into a fresh project; run a case through the plugin with SKILLTEST_BIN
# unset and no `skilltest` on PATH — so a pass can only come from the bundle.
#
#   scripts/smoke-npm-bundle.sh <rust-target> <cli-binary> <fake-provider>
#
# Assumes `pnpm install` has run at the repo root (workspace deps available).
# Quiet on success; on failure the vitest output is the diagnosis.
set -euo pipefail

# Absolutize the binary args against the caller's cwd *before* we chdir, because
# the smoke runs the plugin from a temp consumer project — a relative provider
# path would not resolve from there.
abspath() { case "$1" in /*) printf '%s\n' "$1" ;; *) printf '%s/%s\n' "$PWD" "$1" ;; esac; }

target="${1:-}"
cli_arg="${2:-}"
provider_arg="${3:-}"
if [ -z "$target" ] || [ -z "$cli_arg" ] || [ -z "$provider_arg" ]; then
  echo "error: usage: smoke-npm-bundle.sh <rust-target> <cli-binary> <fake-provider>" >&2
  exit 2
fi
cli="$(abspath "$cli_arg")"
provider="$(abspath "$provider_arg")"

cd "$(dirname "$0")/.."

if [ ! -f "$cli" ] || [ ! -f "$provider" ]; then
  echo "error: cli or provider not found: $cli / $provider" >&2
  exit 2
fi
if command -v skilltest >/dev/null 2>&1; then
  echo "error: a 'skilltest' is on PATH — the smoke could not prove the bundle is used" >&2
  exit 1
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# 1. Stage the binary into the matching platform package and pack all three
#    publishable packages. The platform package packs with `npm` so the binary
#    keeps its executable bit (pnpm pack drops it); the SDK + plugin pack with
#    `pnpm` so their workspace:* deps rewrite to versions (npm would not).
pkgdir="$(bash scripts/stage-npm-binary.sh "$target" "$cli")"
pnpm --filter @skill-test/sdk build >/dev/null
pnpm --filter @skill-test/vitest build >/dev/null
if [ -n "${4:-}" ]; then
  cp "$4"/skill-test-cli-*.tgz "$work/" || {
    echo 'npm bundle smoke: cannot copy staged carrier; check that staging produced a readable tarball and scratch space is writable' >&2
    exit 1
  }
else
  ( cd "$pkgdir" && npm pack --pack-destination "$work" >/dev/null ) || {
    echo "error: carrier packing failed; run npm pack in $pkgdir to reproduce" >&2
    exit 1
  }
fi
( cd sdks/typescript && pnpm pack --pack-destination "$work" >/dev/null )
( cd plugins/vitest && pnpm pack --pack-destination "$work" >/dev/null )

# 2. Fresh consumer project: install the three tarballs + vitest. The non-host
#    platform packages are unpublished, but they are optionalDependencies so npm
#    skips the ones it cannot fetch; the host's is satisfied by its tarball.
proj="$work/proj"
mkdir -p "$proj"
cp -r tests/fixtures/smoke "$proj/smoke"
cat > "$proj/package.json" <<'JSON'
{ "name": "bundle-smoke", "private": true, "type": "module" }
JSON
cat > "$proj/smoke.test.ts" <<'TS'
import { skillTest } from "@skill-test/vitest";

skillTest("bundled binary runs the greeter", "smoke/greet.skilltest.yaml");
TS
( cd "$proj" && npm install --no-audit --no-fund --no-save \
  "$work"/skill-test-sdk-*.tgz \
  "$work"/skill-test-cli-*.tgz \
  "$work"/skill-test-vitest-*.tgz \
  vitest >/dev/null 2>&1 )

# 3. Run the case through the plugin. SKILLTEST_BIN unset + the deterministic fake.
( cd "$proj" && env -u SKILLTEST_BIN SKILLTEST_PROVIDER="$provider" \
  ./node_modules/.bin/vitest run smoke.test.ts )

if ( cd "$proj" && env -u SKILLTEST_BIN SKILLTEST_PROVIDER="$work/missing-provider" \
  ./node_modules/.bin/vitest run smoke.test.ts ) >"$work/failure.log" 2>&1; then
  echo 'error: bundled npm plugin accepted a missing provider; inspect SDK error propagation' >&2
  exit 1
fi
grep -Ei 'provider|ENOENT|not found' "$work/failure.log" >/dev/null || { cat "$work/failure.log" >&2; echo 'npm bundle smoke: missing-provider failure had an unexpected diagnostic; reproduce the plugin case and inspect SDK error propagation' >&2; exit 1; }

echo "npm bundle smoke ($target): ok"
