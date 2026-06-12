#!/usr/bin/env bash
# Bundle smoke (Python): prove a published-shape `skilltest-sdk` wheel runs the
# CLI **bundled inside it**. Build the platform wheel, install it + the pytest
# plugin into a fresh venv, and run a case through the plugin with SKILLTEST_BIN
# unset and no `skilltest` on PATH — so a pass can only come from the bundle.
#
#   scripts/smoke-python-bundle.sh <rust-target> <cli-binary> <fake-provider>
#
# Quiet on success (one line); on failure the pytest output is the diagnosis.
set -euo pipefail
cd "$(dirname "$0")/.."
repo="$PWD"

target="${1:-}"
cli="${2:-}"
provider="${3:-}"
if [ -z "$target" ] || [ ! -f "$cli" ] || [ ! -f "$provider" ]; then
  echo "error: usage: smoke-python-bundle.sh <rust-target> <cli-binary> <fake-provider>" >&2
  exit 2
fi
if command -v skilltest >/dev/null 2>&1; then
  echo "error: a 'skilltest' is on PATH — the smoke could not prove the bundle is used" >&2
  exit 1
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# 1. Platform wheel with the binary bundled.
bash scripts/build-python-wheel.sh "$target" "$cli" "$work/dist" >/dev/null
wheel="$(ls "$work"/dist/skilltest_sdk-*.whl | head -1)"
if [ -z "$wheel" ]; then
  echo "error: no wheel produced for $target" >&2
  exit 1
fi

# 2. Fresh venv: the wheel (skilltest-sdk + bundled binary) and pytest, then the
#    plugin with --no-deps so it reuses the installed wheel, never a source SDK.
uv venv --python 3.12 "$work/venv" >/dev/null
uv pip install --python "$work/venv" "$wheel" pytest >/dev/null
uv pip install --python "$work/venv" --no-deps "$repo/plugins/pytest" >/dev/null

# 3. Run a self-contained case (its own skill, no conftest above it) through the
#    plugin. SKILLTEST_BIN unset + provider pinned to the deterministic fake.
cp -r tests/fixtures/smoke "$work/cases"
env -u SKILLTEST_BIN SKILLTEST_PROVIDER="$provider" \
  "$work/venv/bin/python" -m pytest "$work/cases" -p skilltest_pytest -o addopts="" -q

echo "python bundle smoke ($target): ok"
