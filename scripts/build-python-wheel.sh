#!/usr/bin/env bash
# Build a platform-specific `skilltest-sdk` wheel that bundles the prebuilt CLI.
#
#   scripts/build-python-wheel.sh <rust-target> <skilltest-binary> [out-dir]
#
# Drops the binary at skilltest_sdk/_bin/skilltest (packed via the `artifacts`
# glob in pyproject.toml), builds the wheel, then retags it from py3-none-any to
# the target's platform tag so pip/uv install the matching binary automatically.
# The pure (py3-none-any) wheel + sdist are built separately with an empty _bin
# and serve as the source/unsupported-platform fallback.
#
# Quiet on success (prints the final wheel path); on failure prints the error.
set -euo pipefail
cd "$(dirname "$0")/.."

target="${1:-}"
binary="${2:-}"
outdir="${3:-$PWD/sdks/python/dist}"

# Absolutize outdir: the wheel is built from a `cd sdks/python` subshell, so a
# relative path would resolve against the wrong directory.
case "$outdir" in /*) ;; *) outdir="$PWD/$outdir" ;; esac

if [ -z "$target" ] || [ -z "$binary" ]; then
  echo "error: usage: build-python-wheel.sh <rust-target> <skilltest-binary> [out-dir]" >&2
  exit 2
fi
if [ ! -f "$binary" ]; then
  echo "error: binary not found: $binary" >&2
  exit 2
fi

# Rust target triple -> Python wheel platform tag.
case "$target" in
x86_64-unknown-linux-gnu) plat="manylinux_2_17_x86_64" ;;
aarch64-unknown-linux-gnu) plat="manylinux_2_17_aarch64" ;;
x86_64-apple-darwin) plat="macosx_10_12_x86_64" ;;
aarch64-apple-darwin) plat="macosx_11_0_arm64" ;;
*)
  echo "error: unsupported target: $target" >&2
  exit 2
  ;;
esac

bindir="sdks/python/skilltest_sdk/_bin"
cleanup() { rm -rf "$bindir"; }
trap cleanup EXIT

mkdir -p "$bindir"
install -m 0755 "$binary" "$bindir/skilltest"

# Build the (nominally pure) wheel containing the binary, then relabel its tag.
( cd sdks/python && rm -rf build && uv build --wheel --out-dir "$outdir" >/dev/null )

pure="$(ls "$outdir"/skilltest_sdk-*-py3-none-any.whl 2>/dev/null | head -1 || true)"
if [ -z "$pure" ]; then
  echo "error: expected a py3-none-any wheel in $outdir to retag" >&2
  exit 1
fi
uvx --quiet wheel tags --platform-tag "$plat" --remove "$pure" >/dev/null

final="$(ls "$outdir"/skilltest_sdk-*-py3-none-"$plat".whl 2>/dev/null | head -1 || true)"
if [ -z "$final" ]; then
  echo "error: retag to $plat did not produce a wheel" >&2
  exit 1
fi
echo "built $final"
