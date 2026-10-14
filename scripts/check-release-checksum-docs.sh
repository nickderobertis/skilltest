#!/usr/bin/env bash
# Drift gate for the prose that states the release checksum names: README.md's
# install section, AGENTS.md's release paragraph and release-targets.toml's
# comment on the GitHub Release archives each name every checksum file
# scripts/release-checksums.txt declares, and no other. Each declared line is
# rendered with {asset} as `skilltest-<target>.tar.gz` and {target} as
# `<target>`, the way those docs write them (`.zip.sha256` on Windows is a
# suffix note, not a name of its own).
#
# Usage: check-release-checksum-docs.sh [repo-root]   (default: this checkout)
# `just release-targets-check` (every `just check` tier) runs it, and
# check-release-checksum-docs-test.sh runs it against broken copies.
#
# Quiet on success. On failure it names the file and the checksum name.
set -euo pipefail

err() {
  echo "check-release-checksum-docs: $1" >&2
  exit 1
}

root="${1:-$(dirname "$0")/..}"
declared="$root/scripts/release-checksums.txt"
docs=(README.md AGENTS.md release-targets.toml)
bt='`'

lines="$(grep -vE '^[[:space:]]*(#|$)' "$declared")" ||
  err "could not read a checksum name from $declared; restore scripts/release-checksums.txt"

expected=()
while IFS= read -r line; do
  name="${line//\{asset\}/skilltest-<target>.tar.gz}"
  expected+=("${name//\{target\}/<target>}")
done <<<"$lines"

for doc in "${docs[@]}"; do
  [ -f "$root/$doc" ] || err "$root/$doc is missing; restore it, since it states the release checksum names"
  for name in "${expected[@]}"; do
    grep -Fq "\`$name\`" "$root/$doc" ||
      err "$doc does not name \`$name\`, which scripts/release-checksums.txt declares; state it where $doc lists the release checksums"
  done
  status=0
  stated="$(grep -oE "${bt}[^${bt} ]+\\.sha256${bt}" "$root/$doc")" || status=$?
  [ "$status" -le 1 ] || err "could not read $root/$doc (grep exited $status); check that it is a readable file"
  while IFS= read -r name; do
    name="${name//$bt/}"
    [ -n "$name" ] || continue
    [ "$name" = ".zip.sha256" ] && continue
    known=0
    for want in "${expected[@]}"; do
      [ "$name" = "$want" ] && known=1
    done
    [ "$known" = 1 ] ||
      err "$doc names \`$name\`, which scripts/release-checksums.txt does not declare; write it as one of: ${expected[*]}"
  done <<<"$stated"
done
