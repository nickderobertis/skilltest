#!/usr/bin/env bash
# llmlint: ignore-file[new_code_lands_in_a_project] repo-level release glue that belongs to no Nx package; release.yml runs it on each platform's runner (AGENTS.md: scripts/*.sh are orchestrator-independent glue).
# Proves a downloaded GitHub Release archive is what a consumer can run: its
# checksum matches, it holds the binary at its root, and that binary reports
# the release. release.yml runs it on each target's own runner against the
# asset it just uploaded, so Windows' .zip is proven as published.
#
# Usage: verify-release-archive.sh <dir> <target> <bin> <version>
#   <dir> holds skilltest-<target>.<tar.gz|zip> and its .sha256; <bin> is
#   skilltest or skilltest.exe (a .exe ships as a .zip); <version> is X.Y.Z.
#
# Quiet on success, one line.
set -euo pipefail

err() {
  echo "verify-release-archive: $1" >&2
  exit 1
}

usage="usage: verify-release-archive.sh <dir> <target> <bin> <version>; pass the download directory, a target triple, skilltest or skilltest.exe, and the release version X.Y.Z"
[ $# -eq 4 ] && [ -d "$1" ] || err "$usage"
dir="$1" target="$2" bin="$3" version="$4"
[[ $target =~ ^[A-Za-z0-9_]+(-[A-Za-z0-9_]+)+$ ]] || err "target '$target' is not a target triple; $usage"
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$ ]] || err "version '$version' is not X.Y.Z; $usage"
case "$bin" in
  skilltest.exe) asset="skilltest-$target.zip" ;;
  skilltest) asset="skilltest-$target.tar.gz" ;;
  *) err "bin '$bin' is neither skilltest nor skilltest.exe; $usage" ;;
esac
[ -f "$dir/$asset" ] || err "$dir has no $asset; check release.yml's archive name and that the upload step ran"
[ -f "$dir/$asset.sha256" ] || err "$dir has no $asset.sha256; check release.yml's checksum setting"

if command -v sha256sum >/dev/null 2>&1; then
  hashed="$(sha256sum "$dir/$asset")" || err "sha256sum could not read $dir/$asset; check its permissions and re-download it"
else
  hashed="$(shasum -a 256 "$dir/$asset")" || err "neither sha256sum nor a working shasum could hash $dir/$asset; install coreutils or perl's shasum and rerun"
fi
actual="${hashed%% *}"
recorded="$(tr -d '\r' <"$dir/$asset.sha256")" || err "could not read $dir/$asset.sha256; check its permissions and re-download it"
expected="${recorded%% *}"
[ "$expected" = "$actual" ] ||
  err "checksum mismatch for $asset (its .sha256 says $expected, the archive hashes to $actual); re-run release.yml for $target"

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
  err "$asset holds $(printf '%s' "$members" | tr '\n' ' ' | sed 's/ $//') rather than only $bin at its root, where scripts/install.sh and users look; check release.yml's bin and archive settings"

out="$dir/extracted"
{ rm -rf "$out" && mkdir -p "$out"; } || err "could not prepare $out to extract into; check that $dir is writable"
case "$asset" in
  *.zip) unzip -q "$dir/$asset" -d "$out" || err "could not extract $asset; download it and open it by hand" ;;
  *) tar -xzf "$dir/$asset" -C "$out" || err "could not extract $asset; download it and open it by hand" ;;
esac
{ [ -f "$out/$bin" ] && [ ! -L "$out/$bin" ]; } ||
  err "$asset's $bin is not a regular file; check release.yml's bin setting"

reported="$("$out/$bin" --version 2>&1)" ||
  err "$bin from $asset did not run ($reported); rebuild $target and re-run release.yml"
[ "$reported" = "skilltest $version" ] ||
  err "$bin from $asset reports '$reported', not 'skilltest $version'; the archive is from another build, so re-run release.yml for this tag"

echo "verify-release-archive: $asset runs skilltest $version"
