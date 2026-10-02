#!/usr/bin/env bash
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

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
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

[ -f "$declarations" ] || unanswered "cannot read $declarations, so no identifier can be recognised; run this from a complete checkout"

# The declaration authorizes which registries this probe will read, so it is
# read structurally: the version it was written for, then one id per [[target]]
# block. A bare `id = "..."` line outside a block declares nothing.
#
# What the block boundary is for: only a [[target]] is a release target. A
# [[retired]] entry records an artifact this repository does NOT publish any
# more and carries an `id` of its own, and a target's `covers` list names
# artifacts nothing depends on by name. Answering for either would report a
# version for something no consumer may wait on — so a table header always ends
# the block above it, and a [[target]] that somehow carried no id can never
# reach into the entry below for one.
declared_version="$(sed -n 's/^schema_version = \([0-9]*\)$/\1/p' "$declarations")"
[ "$declared_version" = "$DECLARATION_SCHEMA_VERSION" ] ||
  unanswered "$declarations declares schema_version '$declared_version' and this probe reads exactly one, version $DECLARATION_SCHEMA_VERSION; leave a single schema_version line saying which shape the file is written in"
declared="$(awk '
  /^[[:space:]]*\[/ { inside = ($0 == "[[target]]"); next }
  inside && match($0, /^id = "[^"]+"$/) {
    entry = $0; sub(/^id = "/, "", entry); sub(/"$/, "", entry)
    print entry; inside = 0
  }
' "$declarations")"
[ -n "$declared" ] || unanswered "$declarations declares no release targets; restore its [[target]] entries"

if ! printf '%s\n' "$declared" | grep -Fxq -- "$identifier"; then
  usage_error "'$identifier' is not a release target of this repository, so nothing can be said about it — this is not an answer of 'no release yet'. Declared: $(printf '%s' "$declared" | tr '\n' ' ')"
fi

registry="${identifier%%:*}"
name="${identifier#*:}"

# Per registry: the URL that serves its metadata, and the paths to the version
# it currently serves, most preferred first. Paths are JSON key paths so a key
# holding a hyphen needs no per-reader quoting.
case "$registry" in
  crate)
    # crates.io: ASCII alphanumerics, hyphen and underscore.
    name_syntax='^[A-Za-z0-9][A-Za-z0-9_-]*$'
    url="https://crates.io/api/v1/crates/${name}"
    paths='[["crate","max_stable_version"],["crate","max_version"]]'
    ;;
  pypi)
    # PEP 503: alphanumerics separated by runs of `.`, `_` or `-`.
    name_syntax='^[A-Za-z0-9]([A-Za-z0-9._-]*[A-Za-z0-9])?$'
    url="https://pypi.org/pypi/${name}/json"
    paths='[["info","version"]]'
    ;;
  npm)
    # An optional `@scope/`, then the package name — the one `/` a name may
    # carry. A scoped name is still one path segment, so that separator is
    # percent-encoded rather than left to open a path of its own.
    name_syntax='^(@[A-Za-z0-9][A-Za-z0-9._-]*/)?[A-Za-z0-9][A-Za-z0-9._-]*$'
    url="https://registry.npmjs.org/${name//\//%2f}"
    paths='[["dist-tags","latest"]]'
    ;;
  *)
    usage_error "unknown registry '$registry' in '$identifier'; this probe answers for crate, pypi and npm"
    ;;
esac

# The declaration is a file this probe reads, so the name it carries is an input
# like any other — and this one becomes a path segment of a registry URL.
# Membership in the declaration says the id was written down, not that its name
# is a package name: a name carrying a separator, a query or nothing at all
# would ask the registry a different question than the one declared, and its
# answer would be published as that target's version.
[[ $name =~ $name_syntax ]] ||
  usage_error "'$name' in '$identifier' is not a $registry package name (expected $name_syntax), so no artifact that registry serves is being named; fix that id in $declarations"

command -v curl >/dev/null 2>&1 || unanswered "curl is required to read $registry; install curl and retry"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
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
  cat "$work/curl-error" >&2
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
version=""
if command -v jq >/dev/null 2>&1; then
  version="$( { jq -j --argjson paths "$paths" \
    '[$paths[] as $p | getpath($p)] | map(select(type == "string" and length > 0)) | first // ""' \
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
  cat "$work/read-error" >&2
  unanswered "could not parse the $registry response for '$identifier'; fetch $url yourself and, if it is still JSON, report the reader error above — otherwise that registry's API moved and this probe needs its new URL"
fi

version="${version%X}"

# A 200 carrying no version is an answer this probe does not understand, not an
# artifact that was never released — so it is not answered.
[ -n "$version" ] || unanswered "$registry answered for '$identifier' without a version at $(printf '%s' "$paths"); fetch $url yourself and update this script's \$paths for $registry to wherever it now serves the current version"

# Every registry here serves a version as dot-separated numeric release
# segments with an optional suffix — semver, and PEP 440 with its rare `N!`
# epoch. Requiring that shape is what separates a version from a word a
# registry might serve in its place: `latest` carries nothing a caller can
# order, and `1latest` or `1..` carry less than they look like they do.
#
# The match is against the WHOLE value, line breaks included, because the
# caller is promised ONE line: a line-oriented matcher would accept a multiline
# value on the strength of its first line and then print every line of it.
VERSION_SYNTAX='^[0-9]+(\.[0-9]+)*([.+~!-][0-9A-Za-z][0-9A-Za-z.+_~!-]*)?$'
[[ $version =~ $VERSION_SYNTAX ]] ||
  unanswered "$registry served '$version' for '$identifier', which is not a version a caller can use; fetch $url yourself and point this script's \$paths for $registry at the field carrying the version, or widen the shape it accepts if that registry really serves versions like this"

printf '%s\n' "$version"
