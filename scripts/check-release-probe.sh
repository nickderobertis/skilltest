#!/usr/bin/env bash
# Offline behavioral test of scripts/release-probe.sh, the onevcs release probe
# release-targets.toml names.
#
# The one thing the probe must never do is report a question it could not
# answer as the answer "no release yet": a consumer reads that as a fact about
# the registry and stops waiting. So every path that cannot produce a version is
# driven through the real script and asserted non-zero with a reason on stderr
# and NOTHING on stdout; and the two answers a caller acts on — a version, and
# empty output for a registry 404 — are driven for every declared target in its
# own registry's response shape, so a probe that refused everything cannot pass.
#
# Offline by construction: `curl` is a double on PATH that records each call and
# answers as a case tells it to, through files only it reads. The probe itself
# runs under `env -i` with nothing but PATH and HOME, exactly as onevcs spawns it.
#
# Quiet on success, one line. On failure it prints what the probe said.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

fail() {
  echo "check-release-probe: $1" >&2
  [ -s "$work/err" ] && { echo "  the probe's stderr:" >&2; cat "$work/err" >&2; }
  exit 1
}

# The executable FILE `type -P` resolves for $1, made absolute, so a link to it
# placed in a scratch directory does not dangle.
tool_path() {
  local path
  path="$(type -P "$1")" || return 1
  case $path in
    /*) printf '%s\n' "$path" ;;
    *) printf '%s/%s\n' "$PWD" "$path" ;;
  esac
}

# The curl double. What it answers is set per case through files in $stub,
# never through the probe's environment, which carries only PATH and HOME.
stub="$work/stub"
reached="$stub/reached"
mkdir -p "$work/bin" "$stub"
cat >"$work/bin/curl" <<STUB
#!/usr/bin/env bash
printf 'curl %s\n' "\$*" >>"$reached"
out=""
while [ "\$#" -gt 0 ]; do
  case \$1 in
    --output) out=\$2; shift 2 ;;
    *) shift ;;
  esac
done
if [ -f "$stub/transport-fails" ]; then
  echo "stub: could not resolve host" >&2
  exit 6
fi
if [ -n "\$out" ]; then cat "$stub/body" >"\$out" 2>/dev/null || : >"\$out"; fi
cat "$stub/status" 2>/dev/null || printf 200
STUB
chmod +x "$work/bin/curl"

# $1 = HTTP status, $2 = body. Clears any transport failure a prior case set.
answer() {
  rm -f "$stub/transport-fails"
  printf '%s' "$1" >"$stub/status"
  printf '%b' "$2" >"$stub/body"
}

# A PATH with only what the probe needs before it reads a registry, so a case
# can take away curl or both JSON readers without taking away the shell.
mkdir -p "$work/minbin"
for tool in bash dirname sed awk grep tr head mktemp rm cat; do
  path="$(tool_path "$tool")" ||
    fail "no $tool on this host, so the restricted-PATH cases cannot be built; install $tool (coreutils on Linux and macOS) and rerun"
  ln -s "$path" "$work/minbin/$tool"
done

stub_path="$work/bin:$PATH"

# Runs a probe as onevcs does: one argument list, the repository root as its
# working directory, only PATH and HOME. $1 = PATH, $2 = probe, then its args.
run_probe() {
  local path=$1 script=$2
  shift 2
  : >"$reached"
  env -i PATH="$path" HOME="$HOME" "$script" "$@" >"$work/out" 2>"$work/err"
}

# $1 = description, $2 = the reason the refusal must give, $3 = PATH, $4 =
# probe, then its args. A refusal a caller cannot mistake for the empty answer,
# on the branch the case is about — a case refused for another reason tests
# nothing.
assert_not_answered() {
  local description=$1 reason=$2 path=$3 script=$4 status=0
  shift 4
  run_probe "$path" "$script" "$@" || status=$?
  [ "$status" -ne 0 ] ||
    fail "$description was answered (exit 0) instead of refused; a caller cannot tell it from 'no release yet' — end that branch in scripts/release-probe.sh through unanswered/usage_error"
  [ ! -s "$work/out" ] ||
    fail "$description wrote '$(cat "$work/out")' to stdout; a refusal says nothing there, so route that message to stderr in scripts/release-probe.sh"
  [ -s "$work/err" ] ||
    fail "$description gave no reason on stderr; give that branch of scripts/release-probe.sh an unanswered/usage_error call naming what it could not establish"
  grep -Fq -- "$reason" "$work/err" ||
    fail "$description was refused for some reason other than '$reason', so it exercised a branch it is not about; fix the branch of scripts/release-probe.sh that now swallows it"
}

# Refused from the declaration alone: the network must not be touched.
assert_declined_offline() {
  local description=$1 reason=$2
  shift 2
  assert_not_answered "$description" "$reason" "$stub_path" scripts/release-probe.sh "$@"
  [ ! -s "$reached" ] ||
    fail "$description read the network before refusing; decide it from the declaration alone"
}

# $1 = description, $2 = the exact stdout ("" for the empty answer), $3 = PATH,
# then the probe's arguments.
assert_answered() {
  local description=$1 expected=$2 path=$3 status=0
  shift 3
  run_probe "$path" scripts/release-probe.sh "$@" || status=$?
  [ "$status" -eq 0 ] ||
    fail "$description was refused (exit $status) instead of answered; a caller holds forever on a refusal, so fix the branch of scripts/release-probe.sh that swallows it"
  [ "$(cat "$work/out")" = "$expected" ] ||
    fail "$description answered '$(cat "$work/out")' where a caller is promised '$expected'; scripts/release-probe.sh prints the version and nothing else, or nothing for a registry 404"
  if [ -z "$expected" ] && [ -s "$work/out" ]; then
    fail "$description printed output where the empty answer is promised"
  fi
}

# --- Every declared target: both answers, in its registry's own shape --------
declared="$(awk '
  /^[[:space:]]*\[/ { inside = ($0 == "[[target]]"); next }
  inside && /^id = "[^"]+"$/ { v = $0; sub(/^id = "/, "", v); sub(/"$/, "", v); print v; inside = 0 }
' release-targets.toml)"
[ "$(printf '%s\n' "$declared" | wc -l | tr -d ' ')" -eq 6 ] ||
  fail "release-targets.toml declares $(printf '%s\n' "$declared" | wc -l | tr -d ' ') target ids where this test expects the six skilltest publishes; if that set changed on purpose, update this count with it"

while read -r id; do
  name="${id#*:}"
  case "$id" in
    crate:*)
      body='{"crate":{"name":"'"$name"'","max_stable_version":"0.11.2","max_version":"0.11.2"}}'
      want_url="https://crates.io/api/v1/crates/$name"
      ;;
    pypi:*)
      body='{"info":{"name":"'"$name"'","version":"0.11.2"},"releases":{}}'
      want_url="https://pypi.org/pypi/$name/json"
      ;;
    npm:*)
      body='{"name":"'"$name"'","dist-tags":{"latest":"0.11.2","next":"0.12.0-rc.1"}}'
      want_url="https://registry.npmjs.org/${name//\//%2f}"
      ;;
    *) fail "release-targets.toml declares '$id' on a registry this test has no response shape for; add one here" ;;
  esac
  answer 200 "$body"
  assert_answered "$id with a release" "0.11.2" "$stub_path" "$id"
  grep -Fq -- "$want_url" "$reached" ||
    fail "$id was requested as '$(cat "$reached")' rather than $want_url; the registry URL in scripts/release-probe.sh moved"
  grep -Fq -- "--max-time" "$reached" ||
    fail "$id was read without a --max-time bound, so the probe could outlive the sixty seconds onevcs allows"
  answer 404 '{"errors":[{"detail":"Not Found"}]}'
  assert_answered "$id never released (registry 404)" "" "$stub_path" "$id"
done < <(printf '%s\n' "$declared")

# --- Refused before any registry is read --------------------------------------
assert_declined_offline "no identifier at all" "takes exactly one registry-qualified identifier"
assert_declined_offline "two identifiers" "takes exactly one registry-qualified identifier" \
  crate:skilltest-core crate:skilltest-cli
assert_declined_offline "an unqualified name" "is not a release target of this repository" skilltest-sdk
assert_declined_offline "an unknown registry" "is not a release target of this repository" cargo:skilltest-core
# A real PyPI name skilltest does not publish: PyPI would answer 404, and
# reporting that as "no release yet" would hold a consumer on nothing.
assert_declined_offline "a name this repository does not publish" \
  "is not a release target of this repository" pypi:skilltest
assert_declined_offline "a declared name under the wrong registry" \
  "is not a release target of this repository" npm:skilltest-sdk
# Covered ids are published but nothing may wait on one by name.
assert_declined_offline "a covers entry" \
  "is not a release target of this repository" npm:@skill-test/cli-linux-x64

# Fixture declarations: the probe reads the release-targets.toml beside it.
fixture="$work/fixture"
mkdir -p "$fixture/scripts"
cp scripts/release-probe.sh "$fixture/scripts/release-probe.sh"
fixture_probe="$fixture/scripts/release-probe.sh"
assert_not_answered "a checkout with no release-targets.toml" \
  "cannot read" "$stub_path" "$fixture_probe" crate:skilltest-core
: >"$fixture/release-targets.toml"
assert_not_answered "a declaration with no schema_version" \
  "declares schema_version ''" "$stub_path" "$fixture_probe" crate:skilltest-core
for other in 2 4; do
  printf 'schema_version = %s\n\n[[target]]\nid = "crate:skilltest-core"\n' "$other" >"$fixture/release-targets.toml"
  assert_not_answered "a declaration at schema_version $other" \
    "declares schema_version '$other'" "$stub_path" "$fixture_probe" crate:skilltest-core
done
printf 'schema_version = 3\n' >"$fixture/release-targets.toml"
assert_not_answered "a declaration with no targets" \
  "declares no release targets" "$stub_path" "$fixture_probe" crate:skilltest-core
# Only a [[target]] is a release target; a retired id or a covers entry beside a
# real target is still refused.
cat >"$fixture/release-targets.toml" <<'NEIGHBOURS'
schema_version = 3

[[target]]
id = "npm:@skill-test/sdk"
name = "node-sdk"
covers = ["npm:@skill-test/cli-linux-x64"]

[[retired]]
id = "pypi:skilltest-retired"
why = "Nothing here publishes it again."
NEIGHBOURS
assert_not_answered "a retired id beside a declared target" \
  "is not a release target of this repository" "$stub_path" "$fixture_probe" pypi:skilltest-retired
assert_not_answered "a covered id beside the target that covers it" \
  "is not a release target of this repository" "$stub_path" "$fixture_probe" npm:@skill-test/cli-linux-x64
# A declared name that is not a package name never becomes a URL.
printf 'schema_version = 3\n\n[[target]]\nid = "crate:skilltest/../serde"\n' >"$fixture/release-targets.toml"
assert_not_answered "a malformed declared crate name" \
  "is not a crate package name" "$stub_path" "$fixture_probe" "crate:skilltest/../serde"
printf 'schema_version = 3\n\n[[target]]\nid = "npm:@skill-test/sdk/extra"\n' >"$fixture/release-targets.toml"
assert_not_answered "a malformed declared npm name" \
  "is not a npm package name" "$stub_path" "$fixture_probe" "npm:@skill-test/sdk/extra"
# A registry the probe has no URL for, even when declared.
printf 'schema_version = 3\n\n[[target]]\nid = "gem:skilltest"\n' >"$fixture/release-targets.toml"
assert_not_answered "a declared id on an unknown registry" \
  "unknown registry 'gem'" "$stub_path" "$fixture_probe" gem:skilltest
[ ! -s "$reached" ] ||
  fail "a fixture refusal read the network; every refusal above is decided before any request"

# --- Everything a registry read can do other than answer ----------------------
assert_not_answered "a host with no curl" \
  "curl is required" "$work/minbin" scripts/release-probe.sh pypi:skilltest-sdk
: >"$stub/transport-fails"
assert_not_answered "an unreachable registry" \
  "could not reach" "$stub_path" scripts/release-probe.sh pypi:skilltest-sdk
for status in 500 503 403 301; do
  answer "$status" '{}'
  assert_not_answered "a registry answering $status" \
    "returned HTTP $status" "$stub_path" scripts/release-probe.sh pypi:skilltest-sdk
done
answer 200 'not json at all'
assert_not_answered "an unparseable response" \
  "could not parse" "$stub_path" scripts/release-probe.sh pypi:skilltest-sdk
answer 200 '{"info":{}}'
assert_not_answered "a response with no version" \
  "without a version at" "$stub_path" scripts/release-probe.sh pypi:skilltest-sdk
answer 200 '{"dist-tags":{"latest":""}}'
assert_not_answered "a response with an empty version" \
  "without a version at" "$stub_path" scripts/release-probe.sh npm:@skill-test/sdk
for bad in 'latest' '1latest' '1..' 'see the release notes' '1.2.3\\n' '1.2.3\\nand more'; do
  answer 200 '{"crate":{"max_stable_version":"'"$bad"'"}}'
  assert_not_answered "a response serving '$bad' as the version" \
    "which is not a version a caller can use" "$stub_path" scripts/release-probe.sh crate:skilltest-core
done
answer 200 '{"info":{"version":"0.11.2"}}'
assert_not_answered "a host with neither JSON reader" \
  "neither jq nor python3" "$work/minbin:$work/bin" scripts/release-probe.sh pypi:skilltest-sdk

# --- Registry specifics --------------------------------------------------------
# crates.io serves a prerelease as max_version while max_stable_version holds
# what a dependent may take, so the path order is load-bearing.
answer 200 '{"crate":{"max_stable_version":"0.11.2","max_version":"0.12.0-rc.1"}}'
assert_answered "a crate with a newer prerelease" "0.11.2" "$stub_path" crate:skilltest-cli
answer 200 '{"crate":{"max_stable_version":null,"max_version":"0.1.0-alpha.1"}}'
assert_answered "a crate with only a prerelease" "0.1.0-alpha.1" "$stub_path" crate:skilltest-cli

# Both readers, since either can be the one a host has.
readers=0
for reader in jq python3; do
  path="$(tool_path "$reader")" || continue
  mkdir -p "$work/reader-$reader"
  ln -sf "$path" "$work/reader-$reader/$reader"
  answer 200 '{"info":{"version":"0.11.2"}}'
  assert_answered "the $reader reader" "0.11.2" "$work/reader-$reader:$work/minbin:$work/bin" pypi:skilltest-sdk
  readers=$((readers + 1))
done
[ "$readers" -gt 0 ] ||
  fail "this host has neither jq nor python3, so no reader could be exercised; install one (the probe needs it too) and rerun"

# --- One schema version across the declaration and both of its readers --------
declared_version="$(sed -n 's/^schema_version = \([0-9]*\)$/\1/p' release-targets.toml)"
for reader in scripts/release-probe.sh scripts/check-release-targets.sh; do
  reader_version="$(sed -n 's/^DECLARATION_SCHEMA_VERSION=\([0-9]*\)$/\1/p' "$reader")"
  [ -n "$reader_version" ] && [ "$reader_version" = "$declared_version" ] ||
    fail "$reader reads schema_version '$reader_version' and release-targets.toml declares '$declared_version'; bring whichever is behind up to the other"
done

echo "check-release-probe: all six targets answer a version and a 404's empty answer, and every unanswerable case is refused ($readers reader(s))"
