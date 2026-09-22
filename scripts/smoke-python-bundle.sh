#!/usr/bin/env bash
# Bundle smoke (Python): prove a published-shape `skilltest-sdk` wheel runs the
# CLI **bundled inside it**. Build the platform wheel and the plugin's own wheel,
# install both into a fresh venv, and run a case through the plugin with
# SKILLTEST_BIN unset and no `skilltest` on PATH — so a pass can only come from
# the bundle. Each package is built and installed as its own distribution: the
# uv workspace they share is a development-time resolution, never a publish one.
#
#   scripts/smoke-python-bundle.sh <rust-target> <cli-binary> <fake-provider>
#
# Quiet on success (one line); on failure the pytest output is the diagnosis.
set -euo pipefail

# Absolutize the binary args against the caller's cwd before we chdir, so the
# paths stay valid no matter the working directory the plugin runs from.
abspath() { case "$1" in /*) printf '%s\n' "$1" ;; *) printf '%s/%s\n' "$PWD" "$1" ;; esac; }

target="${1:-}"
cli_arg="${2:-}"
provider_arg="${3:-}"
if [ -z "$target" ] || [ -z "$cli_arg" ] || [ -z "$provider_arg" ]; then
  echo "error: usage: smoke-python-bundle.sh <rust-target> <cli-binary> <fake-provider>" >&2
  echo "hint: against a local debug build, run: scripts/smoke-python-bundle.sh \\" >&2
  echo "      x86_64-unknown-linux-gnu target/debug/skilltest target/debug/skilltest-fake-provider" >&2
  exit 2
fi
cli="$(abspath "$cli_arg")"
provider="$(abspath "$provider_arg")"

cd "$(dirname "$0")/.."
repo="$PWD"

if [ ! -f "$cli" ] || [ ! -f "$provider" ]; then
  echo "error: cli or provider not found: $cli / $provider" >&2
  echo "hint: build them first: cargo build -p skilltest-cli --bin skilltest && \\" >&2
  echo "      cargo build -p skilltest-cli --bin skilltest-fake-provider --features fake-provider" >&2
  exit 2
fi
if command -v skilltest >/dev/null 2>&1; then
  echo "error: a 'skilltest' is on PATH — the smoke could not prove the bundle is used" >&2
  echo "hint: uninstall it, or re-run with a PATH that excludes $(command -v skilltest)." >&2
  exit 1
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# 1. Platform wheel with the binary bundled.
bash scripts/build-python-wheel.sh "$target" "$cli" "$work/dist" >/dev/null
wheel="$(ls "$work"/dist/skilltest_sdk-*.whl 2>/dev/null | head -1 || true)"
if [ -z "$wheel" ]; then
  echo "error: no wheel produced for $target" >&2
  echo "hint: run \`scripts/build-python-wheel.sh $target $cli\` to see the build failure." >&2
  exit 1
fi

# 2. The plugin's own wheel must carry the exact `skilltest-sdk==<version>` pin:
#    the workspace source in plugins/pytest/pyproject.toml resolves the SDK for
#    development only, and a wheel that shipped that instead of the pin would
#    install with no SDK at all.
if ! build_output="$( cd "$repo/plugins/pytest" && uv build --wheel --out-dir "$work/dist" 2>&1 )"; then
  echo "error: building the skilltest-pytest wheel failed" >&2
  echo "$build_output" >&2
  echo "hint: run \`uv build --wheel\` in plugins/pytest to reproduce it." >&2
  exit 1
fi
plugin_wheel="$(ls "$work"/dist/skilltest_pytest-*.whl 2>/dev/null | head -1 || true)"
if [ -z "$plugin_wheel" ]; then
  echo "error: no skilltest-pytest wheel produced" >&2
  echo "hint: run \`uv build --wheel\` in plugins/pytest to see the build failure." >&2
  exit 1
fi

# 3. Fresh venv: the SDK wheel (binary bundled) + pytest, then the plugin wheel
#    with --no-deps so the SDK in play can only be the installed wheel, never
#    the workspace member on disk.
uv venv --python 3.12 "$work/venv" >/dev/null
uv pip install --python "$work/venv" "$wheel" pytest >/dev/null

if ! pin="$("$work/venv/bin/python" - "$plugin_wheel" <<'PYEOF'
import re, sys, zipfile

with zipfile.ZipFile(sys.argv[1]) as zf:
    (name,) = [n for n in zf.namelist() if n.endswith(".dist-info/METADATA")]
    metadata = zf.read(name).decode()
pins = re.findall(r"(?m)^Requires-Dist: (skilltest-sdk==.+)$", metadata)
if not pins:
    sys.exit(1)
print(pins[0])
PYEOF
)"; then
  echo "error: the skilltest-pytest wheel declares no exact skilltest-sdk pin" >&2
  echo "hint: [project].dependencies in plugins/pytest/pyproject.toml must keep" >&2
  echo "      \"skilltest-sdk==<version>\"; [tool.uv.sources] is development-only." >&2
  exit 1
fi

uv pip install --python "$work/venv" --no-deps "$plugin_wheel" >/dev/null

# 4. Run a self-contained case (its own skill, no conftest above it) through the
#    plugin. SKILLTEST_BIN unset + provider pinned to the deterministic fake.
cp -r tests/fixtures/smoke "$work/cases"
env -u SKILLTEST_BIN SKILLTEST_PROVIDER="$provider" \
  "$work/venv/bin/python" -m pytest "$work/cases" -p skilltest_pytest -o addopts="" -q

echo "python bundle smoke ($target): ok (plugin pins $pin)"
