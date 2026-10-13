#!/usr/bin/env bash
# Stage all shipped forms from one compiled binary; never publish here.
#
# Usage: stage-release-product.sh <target> <binary> <output-dir>
# Exit codes: 0 staged and verified; 1 a build, pack or payload check failed;
# 2 invalid invocation.
set -euo pipefail
if [ "$#" -ne 3 ] || [ ! -f "$2" ]; then
  echo 'stage-release-product: usage: <target> <binary> <output-dir>; build the CLI first' >&2
  exit 2
fi
target="$1"
binary_dir="$(cd "$(dirname "$2")" && pwd)" || { echo 'stage-release-product: cannot resolve binary directory; restore the build output and check permissions' >&2; exit 1; }
binary_name="$(basename "$2")" || { echo 'stage-release-product: cannot resolve binary name; pass the built CLI path' >&2; exit 1; }
binary="$binary_dir/$binary_name"
mkdir -p "$3" || { echo 'stage-release-product: cannot create output directory; check its parent permissions' >&2; exit 1; }
out="$(cd "$3" && pwd)" || { echo 'stage-release-product: cannot resolve output directory; check its permissions' >&2; exit 1; }
cd "$(dirname "$0")/.." || { echo 'stage-release-product: cannot enter repository; restore a complete checkout' >&2; exit 1; }
if ! build_output="$(bash scripts/build-python-wheel.sh "$target" "$binary" "$out" 2>&1)"; then
  printf 'stage-release-product: wheel build failed for %s; rerun build-python-wheel.sh
%s
' "$target" "$build_output" >&2
  exit 1
fi
pkg="$(bash scripts/stage-npm-binary.sh "$target" "$binary")" || { echo 'stage-release-product: npm staging failed; rerun stage-npm-binary.sh for the declared target and binary' >&2; exit 1; }
if ! pack_output="$(cd "$pkg" && npm pack --silent --pack-destination "$out" 2>&1)"; then
  printf 'stage-release-product: npm pack failed for %s; rerun npm pack --pack-destination in the staged carrier directory
%s
' "$target ($pkg)" "$pack_output" >&2
  exit 1
fi
python3 scripts/stage-and-verify-release-product.py "$target" "$binary" "$out" "$pkg" || {
  echo "stage-release-product: payload verification failed for $target; install python3 >= 3.11 if it did not start, else rebuild the target and restage" >&2
  exit 1
}
