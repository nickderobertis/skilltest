#!/usr/bin/env bash
# Assemble the full `skilltest-sdk` PyPI dist: one platform wheel per target that
# has a prebuilt binary, plus a pure (py3-none-any) wheel and an sdist as the
# source/unsupported-platform fallback. pip/uv pick the most specific compatible
# artifact, so a supported host gets the bundled binary and everything else falls
# back to `skilltest` on PATH.
#
#   scripts/build-python-dist.sh <binaries-dir> [out-dir]
#
# <binaries-dir> holds one subdir per target named `skilltest-<rust-target>`,
# each containing the `skilltest` binary (the layout of the publish workflow's
# downloaded build artifacts). Missing targets are skipped with a notice.
set -euo pipefail
cd "$(dirname "$0")/.."

bindir="${1:-}"
outdir="${2:-$PWD/sdks/python/dist}"

# Absolutize outdir: the sdist/pure wheel is built from a `cd sdks/python`
# subshell (as is each platform wheel), so a relative path would land nested.
case "$outdir" in /*) ;; *) outdir="$PWD/$outdir" ;; esac

if [ -z "$bindir" ] || [ ! -d "$bindir" ]; then
  echo "error: usage: build-python-dist.sh <binaries-dir> [out-dir]" >&2
  exit 2
fi

targets="x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu x86_64-apple-darwin aarch64-apple-darwin"

rm -rf "$outdir"
mkdir -p "$outdir"

built=0
for target in $targets; do
  binary="$bindir/skilltest-$target/skilltest"
  if [ ! -f "$binary" ]; then
    echo "notice: no binary for $target — skipping its wheel" >&2
    continue
  fi
  bash scripts/build-python-wheel.sh "$target" "$binary" "$outdir"
  built=$((built + 1))
done

# Pure wheel + sdist fallback, built with an empty _bin so they stay py3-none-any.
rm -rf sdks/python/skilltest_sdk/_bin
( cd sdks/python && rm -rf build && uv build --out-dir "$outdir" >/dev/null )

echo "python dist: $built platform wheel(s) + sdist + pure wheel in $outdir"
