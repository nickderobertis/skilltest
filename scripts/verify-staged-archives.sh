#!/usr/bin/env bash
# Proves a staged release-products directory holds a good archive for every
# platform release-platforms.toml declares, and no other release asset, without
# executing any binary: each products/<target>/ archive is checked by
# verify-release-archive.sh --contents-only, and any other .tar.gz, .zip or
# .sha256 under the directory is refused. The platform list is read from
# release-platforms.toml, so the aggregation cannot drift from the declaration.
#
# Usage: verify-staged-archives.sh <products-dir> <version> [--list-assets]
#   --list-assets prints the verified archive and checksum paths, one per line,
#   instead of the summary, for release.yml to upload exactly that set.
#
# Quiet on success, one line (or the asset list).
set -euo pipefail

err() {
  echo "verify-staged-archives: $1" >&2
  exit 1
}

usage="usage: verify-staged-archives.sh <products-dir> <version> [--list-assets]; pass the downloaded release-products directory and the release version X.Y.Z"
{ [ "$#" -eq 2 ] || { [ "$#" -eq 3 ] && [ "$3" = --list-assets ]; }; } && [ -d "$1" ] || err "$usage"
products="${1%/}" version="$2"
here="$(dirname "$0")"

platforms="$(python3 -c '
import sys, tomllib
with open(sys.argv[1], "rb") as f:
    for p in tomllib.load(f)["platform"]:
        print(p["target"], p["bin"])
' "$here/../release-platforms.toml")" || err "could not read the declared platforms from release-platforms.toml; check that python3 (3.11+) is on PATH and the file is valid TOML"
[ -n "$platforms" ] || err "release-platforms.toml declares no platform; restore its [[platform]] entries"

assets=""
while read -r target bin; do
  [ -d "$products/$target" ] || err "$products has no $target directory; the build job for $target did not stage its products, so re-run publish.yml"
  bash "$here/verify-release-archive.sh" "$products/$target" "$target" "$bin" "$version" --contents-only >/dev/null ||
    err "the staged $target archive failed verification (reason above); restage it by re-running publish.yml"
  case "$bin" in *.exe) asset="skilltest-$target.zip" ;; *) asset="skilltest-$target.tar.gz" ;; esac
  assets="$assets$products/$target/$asset
$products/$target/$asset.sha256
"
done <<<"$platforms"

found="$(find "$products" -type f \( -name '*.tar.gz' -o -name '*.zip' -o -name '*.sha256' \) | sort)" ||
  err "could not list $products; check its permissions"
while read -r file; do
  [ -n "$file" ] || continue
  grep -Fxq -- "$file" <<<"$assets" ||
    err "$file is not a declared release asset; only release-platforms.toml's archives and checksums may be uploaded, so remove it or declare its platform"
done <<<"$found"

if [ "${3:-}" = --list-assets ]; then
  printf '%s' "$assets"
else
  echo "verify-staged-archives: $(wc -l <<<"$platforms" | tr -d ' ') declared archives verified; binaries not executed"
fi
