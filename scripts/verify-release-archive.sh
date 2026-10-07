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

[ $# -eq 4 ] && [ -d "$1" ] && [ -n "$2" ] && [ -n "$3" ] && [ -n "$4" ] ||
  err "usage: verify-release-archive.sh <dir> <target> <bin> <version>; pass the download directory, the target triple, the binary name and the release version"
dir=$1 target=$2 bin=$3 version=$4

case $bin in
  *.exe) asset="skilltest-$target.zip" ;;
  *) asset="skilltest-$target.tar.gz" ;;
esac
[ -f "$dir/$asset" ] || err "$dir has no $asset; check release.yml's archive name and that the upload step ran"
[ -f "$dir/$asset.sha256" ] || err "$dir has no $asset.sha256; check release.yml's checksum setting"

if command -v sha256sum >/dev/null 2>&1; then
  actual="$(sha256sum "$dir/$asset" | awk '{print $1}')"
else
  actual="$(shasum -a 256 "$dir/$asset" | awk '{print $1}')"
fi
expected="$(tr -d '\r' <"$dir/$asset.sha256" | awk '{print $1}')"
[ "$expected" = "$actual" ] ||
  err "checksum mismatch for $asset (its .sha256 says $expected, the archive hashes to $actual); re-run release.yml for $target"

out="$dir/extracted"
rm -rf "$out" && mkdir -p "$out"
case $asset in
  *.zip)
    if command -v unzip >/dev/null 2>&1; then
      unzip -q "$dir/$asset" -d "$out" || err "could not extract $asset; download it and open it by hand"
    else
      pwsh -NoProfile -Command "Expand-Archive -LiteralPath '$dir/$asset' -DestinationPath '$out'" ||
        err "could not extract $asset; download it and open it by hand"
    fi
    ;;
  *) tar -xzf "$dir/$asset" -C "$out" || err "could not extract $asset; download it and open it by hand" ;;
esac
[ -f "$out/$bin" ] || err "$asset holds no $bin at its root, where scripts/install.sh and users look; check release.yml's bin setting"

reported="$("$out/$bin" --version 2>&1)" ||
  err "$bin from $asset did not run ($reported); rebuild $target and re-run release.yml"
[ "$reported" = "skilltest $version" ] ||
  err "$bin from $asset reports '$reported', not 'skilltest $version'; the archive is from another build, so re-run release.yml for this tag"

echo "verify-release-archive: $asset runs skilltest $version"
