#!/usr/bin/env bash
# llmlint: ignore-file[new_code_lands_in_a_project] the probe onevcs runs by the path release-targets.toml declares, from the repository root, standalone under env -i; it is repo-level release glue that belongs to no Nx package (AGENTS.md: scripts/*.sh are orchestrator-independent glue).
# What a registry currently serves for one artifact this repository releases.
#
# A consumer sequencing work across repositories needs to know when a change has
# actually been released, and it reads that from this one script rather than
# from a registry API it would have to learn per registry. The contract is the
# same in every repository that carries one:
#
#   scripts/release-probe.sh <registry>:<name>
#
#     exit 0, one line on stdout  — the version that registry serves right now
#     exit 0, empty stdout        — the registry has no release of it yet
#     non-zero, reason on stderr  — NOT ANSWERED
#
# "Not answered" and "no release yet" are different answers and stay different
# all the way out: a caller holds indefinitely on the first and must never read
# it as evidence that a release has not happened. So every uncertainty resolves
# toward not-answered — an unreachable registry, an unreadable response, a
# response whose version field is missing, and an identifier this repository
# does not declare (a mistyped name is not a package that was never released).
#
# It may assume only what the contract gives it: this repository's root as its
# working directory, and an environment carrying PATH and HOME. Every target is
# on a public registry, so it reads unauthenticated and takes no credential.
# It answers well inside sixty seconds, or says it could not.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)" || {
  printf 'release-probe: cannot resolve the repository root from %s; run it from a complete checkout\n' "${BASH_SOURCE[0]}" >&2
  exit 1
}
declarations="$repo_root/release-targets.toml"
DECLARATION_SCHEMA_VERSION=3

# A usage error: the caller asked something that cannot be answered about.
usage_error() {
  printf 'release-probe: %s\n' "$1" >&2
  exit 2
}

# Could not establish either answer. Never confusable with "no release yet",
# which is exit 0 with no output.
unanswered() {
  printf 'release-probe: %s\n' "$1" >&2
  exit 1
}

[ "$#" -eq 1 ] || usage_error "takes exactly one registry-qualified identifier (<registry>:<name>), got $#; e.g. 'scripts/release-probe.sh pypi:skilltest-sdk'"
identifier="$1"

[ -f "$declarations" ] && [ -r "$declarations" ] || unanswered "cannot read $declarations, so no identifier can be recognised; run this from a complete checkout"

# Only a [[target]]'s own id is answerable: a [[retired]] id or a `covers` entry
# names something no consumer may wait on, so every table header ends the block
# above it and an id outside a [[target]] declares nothing.
# llmlint: ignore-block[boundary_inputs_validated] This is an allowlist read, not the declaration's validation: onevcs validates release-targets.toml in full (`onevcs release declaration`) before it ever spawns this probe, and the probe must run standalone under `env -i` with no TOML library, so it accepts only the exact `schema_version = N` and `id = "..."` lines it can place and refuses everything else as unanswered.
declared_version="$(sed -n 's/^schema_version = \([0-9]*\)$/\1/p' "$declarations")" ||
  unanswered "could not read $declarations; check its permissions and retry"
[ "$declared_version" = "$DECLARATION_SCHEMA_VERSION" ] ||
  unanswered "$declarations declares schema_version '$declared_version' and this probe reads exactly one, version $DECLARATION_SCHEMA_VERSION; leave a single schema_version line saying which shape the file is written in"
# A [[target]] writing `id` twice is refused whole: which of the two a reader
# would take is exactly what nobody wrote down.
declared="$(awk '
  /^[[:space:]]*\[/ { inside = ($0 == "[[target]]"); ids = 0; next }
  inside && /^id[[:space:]]*=/ && ++ids > 1 { print "!duplicate"; exit }
  inside && ids == 1 && match($0, /^id = "[^"]+"$/) {
    entry = $0; sub(/^id = "/, "", entry); sub(/"$/, "", entry)
    print entry
  }
' "$declarations")" || unanswered "could not read $declarations; check its permissions and retry"
case $'\n'"$declared"$'\n' in
  *$'\n!duplicate\n'*) unanswered "$declarations writes id twice in one [[target]]; keep the one that names the artifact and run 'onevcs release declaration .' to validate the rest" ;;
esac
[ -n "$declared" ] || unanswered "$declarations declares no release targets; restore its [[target]] entries"
# llmlint: ignore-end[boundary_inputs_validated]

if ! printf '%s\n' "$declared" | grep -Fxq -- "$identifier"; then
  usage_error "'$identifier' is not a release target of this repository, so nothing can be said about it — this is not an answer of 'no release yet'. Declared: $(printf '%s' "$declared" | tr '\n' ' ')"
fi

registry="${identifier%%:*}"
name="${identifier#*:}"

# Per registry: the URL that serves its metadata, the paths to the version it
# currently serves (most preferred first, as JSON key paths so a key holding a
# hyphen needs no per-reader quoting), and the shape that version takes there.
#
# What each registry serves is matched against the WHOLE value, line breaks
# included, because the caller is promised ONE line. crates.io and npm serve
# semver; PyPI serves the PEP 440 normal form (`0.12.0rc1`, `.post1`, `.dev2`,
# an `N!` epoch, a lowercase local label), never semver's `-rc.1`. Requiring
# that shape is what separates a version from a word served in its place:
# `latest` carries nothing a caller can order, nor do `1latest` or `1..`.
# llmlint: ignore-block[contracts_have_one_source_or_a_drift_gate] Both grammars are transcribed from their specifications — semver.org's published regex, and PEP 440's normalized public form plus local label — which are prose and regex documents rather than code a standalone Bash probe could derive from; scripts/check-release-probe.sh holds each to the spec's own valid and invalid examples.
# semver.org's own grammar: no leading zero in a numeric core or prerelease part.
num='(0|[1-9][0-9]*)'
pre="($num|[0-9]*[A-Za-z-][0-9A-Za-z-]*)"
semver="^$num\\.$num\\.$num(-$pre(\\.$pre)*)?(\\+[0-9A-Za-z-]+(\\.[0-9A-Za-z-]+)*)?\$"
pep440='^([0-9]+!)?[0-9]+(\.[0-9]+)*((a|b|rc)[0-9]+)?(\.post[0-9]+)?(\.dev[0-9]+)?(\+[a-z0-9]+(\.[a-z0-9]+)*)?$'
# llmlint: ignore-end[contracts_have_one_source_or_a_drift_gate]
# llmlint: ignore-block[contracts_have_one_source_or_a_drift_gate] The authority for each URL and field is the live registry, which publishes no schema the offline gate could read (AGENTS.md: no network or non-determinism in `just check`); `just release-probe-live` (scripts/release-probe-live.sh) is the reconciliation, driving this exact case against crates.io, PyPI and npm.
case "$registry" in
  crate)
    # crates.io: ASCII alphanumerics, hyphen and underscore.
    name_syntax='^[A-Za-z0-9][A-Za-z0-9_-]*$'
    url="https://crates.io/api/v1/crates/${name}"
    paths='[["crate","max_stable_version"],["crate","max_version"]]'
    version_syntax=$semver
    ;;
  pypi)
    # PEP 503: alphanumerics separated by runs of `.`, `_` or `-`.
    name_syntax='^[A-Za-z0-9]([A-Za-z0-9._-]*[A-Za-z0-9])?$'
    url="https://pypi.org/pypi/${name}/json"
    paths='[["info","version"]]'
    version_syntax=$pep440
    ;;
  npm)
    # An optional `@scope/`, then the package name — the one `/` a name may
    # carry. A scoped name is still one path segment, so that separator is
    # percent-encoded rather than left to open a path of its own.
    name_syntax='^(@[A-Za-z0-9][A-Za-z0-9._-]*/)?[A-Za-z0-9][A-Za-z0-9._-]*$'
    url="https://registry.npmjs.org/${name//\//%2f}"
    paths='[["dist-tags","latest"]]'
    version_syntax=$semver
    ;;
  *)
    usage_error "unknown registry '$registry' in '$identifier'; this probe answers for crate, pypi and npm"
    ;;
esac
# llmlint: ignore-end[contracts_have_one_source_or_a_drift_gate]

# Being declared does not make a name safe as a registry URL path segment.
[[ $name =~ $name_syntax ]] ||
  usage_error "'$name' in '$identifier' is not a $registry package name (expected $name_syntax), so no artifact that registry serves is being named; fix that id in $declarations"

command -v curl >/dev/null 2>&1 || unanswered "curl is required to read $registry; install curl and retry"

work="$(mktemp -d)" ||
  unanswered "could not create a scratch directory for the $registry response; check that \$TMPDIR (or /tmp) is writable and has space, then retry"
trap 'rm -rf "$work" || printf "release-probe: could not remove %s; delete it by hand\n" "$work" >&2' EXIT
response="$work/response.json"

# Bounded well inside the sixty seconds the contract allows: at most three
# attempts, none longer than fifteen seconds, none started after thirty.
# crates.io refuses a request that does not identify its caller.
status=0
if ! status="$(curl --silent --show-error --location \
  --output "$response" --write-out '%{http_code}' \
  --max-time 15 --retry 2 --retry-delay 1 --retry-max-time 30 \
  --header 'Accept: application/vnd.npm.install-v1+json, application/json' \
  --user-agent 'skilltest-release-probe (https://github.com/nickderobertis/skilltest)' \
  "$url" 2>"$work/curl-error")"; then
  cat "$work/curl-error" >&2 || printf 'release-probe: (curl error output unreadable)\n' >&2
  unanswered "could not reach $url for '$identifier'; retry when the registry is reachable"
fi

case "$status" in
  200) ;;
  404)
    # The registry answered, and its answer is that this artifact has no
    # release. That is the empty answer, and the only thing that produces it.
    exit 0
    ;;
  *)
    unanswered "$registry returned HTTP $status for '$identifier'; retry when the registry recovers"
    ;;
esac

# jq first, python3 second: a host is overwhelmingly likely to carry one, and a
# hand-rolled scan of registry JSON is exactly the kind of confident wrong
# answer this probe exists not to give.
# Both readers write the value and nothing else (jq's -j suppresses the newline
# it would otherwise add), then close with a sentinel byte that is dropped
# below — so a trailing newline the registry really served survives command
# substitution's stripping and is validated rather than silently tidied away.
# Both also read a path that runs through a non-object as "no version there",
# so the answer to one response does not depend on which reader a host has.
version=""
if command -v jq >/dev/null 2>&1; then
  version="$( { jq -j --argjson paths "$paths" \
    '[$paths[] as $p | (try getpath($p) catch null)] | map(select(type == "string" and length > 0)) | first // ""' \
    < "$response" 2>"$work/read-error" && printf X; } )" || version="__unreadable__"
elif command -v python3 >/dev/null 2>&1; then
  version="$( { python3 -c '
import json, sys

paths = json.loads(sys.argv[1])
document = json.load(sys.stdin)
for path in paths:
    value = document
    for key in path:
        if not isinstance(value, dict):
            value = None
            break
        value = value.get(key)
    if isinstance(value, str) and value:
        sys.stdout.write(value)
        break
' "$paths" < "$response" 2>"$work/read-error" && printf X; } )" || version="__unreadable__"
else
  unanswered "neither jq nor python3 is available to read the $registry response; install one and retry"
fi

if [ "$version" = "__unreadable__" ]; then
  cat "$work/read-error" >&2 || printf 'release-probe: (reader error output unreadable)\n' >&2
  unanswered "could not parse the $registry response for '$identifier'; fetch $url yourself and, if it is still JSON, report the reader error above — otherwise that registry's API moved and this probe needs its new URL"
fi

version="${version%X}"

# A 200 carrying no version is an answer this probe does not understand, not an
# artifact that was never released — so it is not answered.
[ -n "$version" ] || unanswered "$registry answered for '$identifier' without a version at $(printf '%s' "$paths"); fetch $url yourself and update this script's \$paths for $registry to wherever it now serves the current version"

[[ $version =~ $version_syntax ]] ||
  unanswered "$registry served '$version' for '$identifier', which is not a $registry version a caller can use; fetch $url yourself and point this script's \$paths for $registry at the field carrying the version, or widen the shape it accepts if that registry really serves versions like this"

printf '%s\n' "$version"
