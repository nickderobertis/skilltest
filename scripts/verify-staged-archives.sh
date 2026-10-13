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
# Quiet on success, one line (or the asset list). Exit codes: 0 verified;
# 1 a declared archive is missing or bad, or an undeclared asset is staged;
# 2 invalid invocation.
set -euo pipefail

err() {
  echo "verify-staged-archives: $1" >&2
  exit 1
}

usage="usage: verify-staged-archives.sh <products-dir> <version> [--list-assets]; pass the downloaded release-products directory and the release version X.Y.Z"
{ [ "$#" -eq 2 ] || { [ "$#" -eq 3 ] && [ "$3" = --list-assets ]; }; } && [ -d "$1" ] || {
  echo "verify-staged-archives: $usage" >&2
  exit 2
}
products="${1%/}" version="$2"
here="$(dirname "$0")" || err "could not resolve this script's directory from $0; run it as bash scripts/verify-staged-archives.sh from the repository root"

# Each target names a products/ directory, so only a well-formed, unique target
# triple and a known binary name are accepted before any path is built.
platforms="$(python3 -c '
import re, sys, tomllib
with open(sys.argv[1], "rb") as f:
    records = tomllib.load(f)["platform"]
seen = set()
for p in records:
    target, bin = p.get("target"), p.get("bin")
    if not isinstance(target, str) or not re.fullmatch(r"[A-Za-z0-9_]+(-[A-Za-z0-9_]+)+", target) or target in seen:
        sys.exit(f"invalid or duplicate target {target!r}")
    if bin not in ("skilltest", "skilltest.exe"):
        sys.exit(f"invalid bin {bin!r} for {target}")
    seen.add(target)
    print(target, bin)
' "$here/../release-platforms.toml")" || err "could not read valid declared platforms from release-platforms.toml (reason above); fix its [[platform]] target/bin fields, and check that python3 (3.11+) is on PATH"
[ -n "$platforms" ] || err "release-platforms.toml declares no platform; restore its [[platform]] entries"

assets="" count=0
while read -r target bin; do
  [ -d "$products/$target" ] || err "$products has no $target directory; the build job for $target did not stage its products, so re-run publish.yml"
  bash "$here/verify-release-archive.sh" "$products/$target" "$target" "$bin" "$version" --contents-only >/dev/null ||
    err "the staged $target archive failed verification (reason above); restage it by re-running publish.yml"
  case "$bin" in *.exe) asset="skilltest-$target.zip" ;; *) asset="skilltest-$target.tar.gz" ;; esac
  assets="$assets$products/$target/$asset
$products/$target/$asset.sha256
"
  count=$((count + 1))
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
  echo "verify-staged-archives: $count declared archives verified; binaries not executed"
fi
