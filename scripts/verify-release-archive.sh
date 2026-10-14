#!/usr/bin/env bash
# llmlint: ignore-file[new_code_lands_in_a_project] repo-level release glue that belongs to no Nx package; release.yml runs it on each platform's runner (AGENTS.md: scripts/*.sh are orchestrator-independent glue).
# Proves a downloaded GitHub Release archive is what a consumer can run: its
# checksum matches, it holds the binary at its root, and that binary reports
# the release. release.yml runs it on each target's own runner against the
# asset it just uploaded, so Windows' .zip is proven as published.
#
# Usage: verify-release-archive.sh <dir> <target> <bin> <version> [--contents-only]
#        verify-release-archive.sh --checksum-names <target> <bin>
#   <dir> holds skilltest-<target>.<tar.gz|zip> and every checksum file
#   scripts/release-checksums.txt names beside it; <bin> is skilltest or
#   skilltest.exe (a .exe ships as a .zip); <version> is X.Y.Z.
#   --checksum-names prints those checksum file names, one per line, and is
#   how the other release scripts read the declaration.
#
# Quiet on success, one line. Exit codes: 0 verified; 1 the archive is
# missing, wrong or does not run; 2 invalid invocation.
set -euo pipefail

err() {
  echo "verify-release-archive: $1" >&2
  exit 1
}

usage="usage: verify-release-archive.sh <dir> <target> <bin> <version> [--contents-only], or --checksum-names <target> <bin>; pass the download directory, a target triple, skilltest or skilltest.exe, and the release version X.Y.Z"
usage_err() {
  echo "verify-release-archive: $1" >&2
  exit 2
}
if [ "${1:-}" = --checksum-names ]; then
  [ "$#" -eq 3 ] || usage_err "$usage"
  names_only=1 target="$2" bin="$3"
else
  { [ "$#" -eq 4 ] || { [ "$#" -eq 5 ] && [ "$5" = --contents-only ]; }; } && [ -d "$1" ] || usage_err "$usage"
  names_only="" dir="$1" target="$2" bin="$3" version="$4"
  [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$ ]] || usage_err "version '$version' is not X.Y.Z; $usage"
fi
[[ "$target" =~ ^[A-Za-z0-9_]+(-[A-Za-z0-9_]+)+$ ]] || usage_err "target '$target' is not a target triple; $usage"
case "$bin" in
  skilltest.exe) asset="skilltest-$target.zip" ;;
  skilltest) asset="skilltest-$target.tar.gz" ;;
  *) usage_err "bin '$bin' is neither skilltest nor skilltest.exe; $usage" ;;
esac

# The checksum file names, from the one declaration beside this script.
here="${BASH_SOURCE[0]%/*}"
[ "$here" != "${BASH_SOURCE[0]}" ] || here=.
declared="$here/release-checksums.txt"
checksums=()
while IFS= read -r line || [ -n "$line" ]; do
  line="${line%$'\r'}"
  case "$line" in '' | '#'*) continue ;; esac
  line="${line//\{asset\}/$asset}"
  checksums+=("${line//\{target\}/$target}")
done <"$declared" || err "could not read $declared; restore it from a complete checkout"
[ "${#checksums[@]}" -gt 0 ] || err "$declared names no checksum file; restore its file-name lines"
if [ -n "$names_only" ]; then
  printf '%s\n' "${checksums[@]}"
  exit 0
fi

[ -f "$dir/$asset" ] || err "$dir has no $asset; check release.yml's archive name and that the upload step ran"
for name in "${checksums[@]}"; do
  [ -f "$dir/$name" ] || err "$dir has no $name; stage it with scripts/stage-and-verify-release-product.py, which writes every name scripts/release-checksums.txt declares"
done

if command -v sha256sum >/dev/null 2>&1; then
  hashed="$(sha256sum "$dir/$asset")" || err "sha256sum could not hash $dir/$asset; check its permissions and re-download it"
else
  hashed="$(shasum -a 256 "$dir/$asset")" || err "shasum could not hash $dir/$asset, and sha256sum is not on PATH; check the archive's permissions, or install coreutils' sha256sum, and rerun"
fi
actual="${hashed%% *}"
for name in "${checksums[@]}"; do
  recorded="$(tr -d '\r' <"$dir/$name")" || err "could not read $dir/$name; check its permissions and re-download it"
  expected="${recorded%% *}"
  [ "$expected" = "$actual" ] ||
    err "checksum mismatch for $asset (its $name says $expected, the archive hashes to $actual); re-run release.yml for $target"
  [ "$recorded" = "$actual  $asset" ] ||
    err "$name does not hold the one line '$actual  $asset'; restage it with scripts/stage-and-verify-release-product.py"
done

# The archive must hold exactly the binary at its root, checked from its
# listing before anything is written, so no entry can land outside $out.
case "$asset" in
  *.zip)
    command -v unzip >/dev/null 2>&1 || err "unzip is not on PATH, so $asset cannot be opened; install unzip (Git for Windows' bash ships it) and rerun"
    members="$(unzip -Z1 "$dir/$asset" 2>&1)" || err "could not list $asset ($members); download it and open it by hand"
    ;;
  *) members="$(tar -tzf "$dir/$asset" 2>&1)" || err "could not list $asset ($members); download it and open it by hand" ;;
esac
[ "$members" = "$bin" ] ||
  err "$asset holds ${members//$'\n'/ } rather than only $bin at its root, where scripts/install.sh and users look; check release.yml's bin and archive settings"

out="$dir/extracted"
{ rm -rf "$out" && mkdir -p "$out"; } || err "could not prepare $out to extract into; check that $dir is writable"
case "$asset" in
  *.zip) unzip -q "$dir/$asset" -d "$out" || err "could not extract $asset; download it and open it by hand" ;;
  *) tar -xzf "$dir/$asset" -C "$out" || err "could not extract $asset; download it and open it by hand" ;;
esac
{ [ -f "$out/$bin" ] && [ ! -L "$out/$bin" ]; } ||
  err "$asset's $bin is not a regular file; check release.yml's bin setting"

if [ "${5:-}" = --contents-only ]; then
  echo "verify-release-archive: $asset checksum and contents verified; binary not executed"
  exit 0
fi

reported="$("$out/$bin" --version 2>&1)" ||
  err "$bin from $asset did not run ($reported); rebuild $target and re-run release.yml"
[ "$reported" = "skilltest $version" ] ||
  err "$bin from $asset reports '$reported', not 'skilltest $version'; the archive is from another build, so re-run release.yml for this tag"

echo "verify-release-archive: $asset runs skilltest $version"
