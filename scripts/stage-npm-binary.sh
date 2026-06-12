#!/usr/bin/env bash
# Stage a prebuilt `skilltest` binary into its per-platform npm package so the
# package can be packed/published with the binary inside.
#
#   scripts/stage-npm-binary.sh <rust-target> <skilltest-binary>
#
# Copies the binary to sdks/typescript/platforms/<pkg>/bin/skilltest (the path
# the SDK's bundledBin() resolves) and prints the package directory. The bin/
# dir is git-ignored; it exists only for a pack/publish. Quiet on success.
set -euo pipefail
cd "$(dirname "$0")/.."

target="${1:-}"
binary="${2:-}"

if [ -z "$target" ] || [ -z "$binary" ]; then
  echo "error: usage: stage-npm-binary.sh <rust-target> <skilltest-binary>" >&2
  exit 2
fi
if [ ! -f "$binary" ]; then
  echo "error: binary not found: $binary" >&2
  exit 2
fi

# Rust target triple -> npm platform package directory (@skill-test/cli-<os>-<arch>).
case "$target" in
x86_64-unknown-linux-gnu) pkg="cli-linux-x64" ;;
aarch64-unknown-linux-gnu) pkg="cli-linux-arm64" ;;
x86_64-apple-darwin) pkg="cli-darwin-x64" ;;
aarch64-apple-darwin) pkg="cli-darwin-arm64" ;;
*)
  echo "error: unsupported target: $target" >&2
  exit 2
  ;;
esac

dir="sdks/typescript/platforms/$pkg"
if [ ! -f "$dir/package.json" ]; then
  echo "error: missing platform package: $dir" >&2
  exit 2
fi

mkdir -p "$dir/bin"
install -m 0755 "$binary" "$dir/bin/skilltest"
echo "$dir"
