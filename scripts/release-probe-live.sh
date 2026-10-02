#!/usr/bin/env bash
# llmlint: ignore-file[new_code_lands_in_a_project] repo-level release glue beside scripts/release-probe.sh, which onevcs runs by its declared path from the repository root; it belongs to no Nx package (AGENTS.md: scripts/*.sh are orchestrator-independent glue).
#
# Live drift alarm for scripts/release-probe.sh: drives it against the REAL
# crates.io, PyPI and npm for every [[target]] in release-targets.toml, the way
# onevcs spawns it (repository root, only PATH and HOME). The offline test
# (scripts/check-release-probe.sh) answers from response shapes written here;
# this is what notices when a registry stops serving the URL or field the probe
# reads. Every skilltest target has been released, so each must answer a
# version — an empty answer means the URL the probe builds no longer exists.
#
# Network-touching, so never in `just check`; run it with `just release-probe-live`.
# Quiet on success, one line.
set -euo pipefail
cd "$(dirname "$0")/.." || {
  echo "release-probe-live: cannot enter the repository root from $0; run it from a complete checkout" >&2
  exit 1
}

# Every [[target]] must yield exactly one id, so an entry the extractor cannot
# read is a failure here rather than a target silently left unprobed.
ids="$(awk '
  /^[[:space:]]*\[/ { if (inside && !found) { print "!unread " NR; exit } inside = ($0 == "[[target]]"); found = 0; next }
  inside && /^id = "[^"]+"$/ { v = $0; sub(/^id = "/, "", v); sub(/"$/, "", v); print v; found = 1; inside = 0 }
  END { if (inside && !found) print "!unread " NR }
' release-targets.toml)" || {
  echo "release-probe-live: cannot read release-targets.toml; run this from a complete checkout" >&2
  exit 1
}

case $ids in
  *'!unread'*)
    echo "release-probe-live: a [[target]] in release-targets.toml (ending near line ${ids##*!unread }) has no id line of the form id = \"<registry>:<name>\"; fix it, then run 'onevcs release declaration .' to validate the rest" >&2
    exit 1
    ;;
esac
[ -n "$ids" ] || {
  echo "release-probe-live: release-targets.toml yielded no [[target]] ids, so nothing would be probed; restore its [[target]] entries" >&2
  exit 1
}

fails=0
answers=""
while read -r id; do
  [ -n "$id" ] || continue
  status=0
  version="$(env -i PATH="$PATH" HOME="$HOME" scripts/release-probe.sh "$id")" || status=$?
  if [ "$status" -ne 0 ]; then
    echo "release-probe-live: $id was not answered (exit $status, reason above); if the registry is up, its API moved — update that registry's url/paths in scripts/release-probe.sh and the response shape in scripts/check-release-probe.sh" >&2
    fails=$((fails + 1))
  elif [ -z "$version" ]; then
    echo "release-probe-live: $id answered 'no release yet', but it has been published; the URL scripts/release-probe.sh builds for it no longer finds it — check that registry's url= line there" >&2
    fails=$((fails + 1))
  else
    answers="$answers $id=$version"
  fi
done <<<"$ids"

[ "$fails" -eq 0 ] || exit 1
echo "release-probe-live:$answers"
