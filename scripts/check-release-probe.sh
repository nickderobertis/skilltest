#!/usr/bin/env bash
# llmlint: ignore-file[new_code_lands_in_a_project] tests scripts/release-probe.sh, repo-level release glue that belongs to no Nx package; run workspace-wide from `just check` (AGENTS.md: scripts/*.sh are orchestrator-independent glue).
# Offline test of scripts/release-probe.sh. Every declared target must answer a
# version and a 404's empty answer; anything uncertain must exit non-zero with
# a reason and no stdout, because a caller reads empty output as "not released".
# curl is a double on PATH; the probe runs under `env -i` with PATH and HOME,
# as onevcs spawns it.
#
# Quiet on success, one line. On failure it prints what the probe said.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.." || {
  echo "check-release-probe: cannot enter the repository root from ${BASH_SOURCE[0]}; run it from a complete checkout" >&2
  exit 1
}

work="$(mktemp -d)" || {
  echo "check-release-probe: could not create a scratch directory; check that \$TMPDIR (or /tmp) is writable and has space" >&2
  exit 1
}
trap 'rm -rf "$work" || echo "check-release-probe: could not remove $work; delete it by hand" >&2' EXIT

fail() {
  echo "check-release-probe: $1" >&2
  if [ -s "$work/err" ]; then
    echo "  the probe's stderr:" >&2
    cat "$work/err" >&2 || echo "  (unreadable: $work/err)" >&2
  fi
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

# A scratch write the harness needs; its failure is the harness's, never a
# probe refusal. $1 = path, then printf's format and arguments.
put() {
  local path=$1
  shift
  # shellcheck disable=SC2059 # the caller passes printf's format on purpose
  printf "$@" >"$path" || fail "could not write $path; check that $work is writable and has space"
}

# The curl double. What it answers is set per case through files in $stub,
# never through the probe's environment, which carries only PATH and HOME. A
# case that reaches it without having said what to answer is a harness bug, so
# it exits with a message naming that rather than inventing a 200.
stub="$work/stub"
reached="$stub/reached"
mkdir -p "$work/bin" "$stub" || fail "could not create the curl double's directories under $work; check that \$TMPDIR (or /tmp) is writable and has space, then rerun"
put "$work/bin/curl" '%s\n' "#!/usr/bin/env bash
set -euo pipefail
printf 'curl %s\\n' \"\$*\" >>'$reached' || { echo 'curl double: could not log this call' >&2; exit 98; }
out=''
while [ \"\$#\" -gt 0 ]; do
  case \$1 in
    --output) out=\$2; shift 2 ;;
    *) shift ;;
  esac
done
if [ -f '$stub/transport-fails' ]; then
  echo 'stub: could not resolve host' >&2
  exit 6
fi
[ -f '$stub/status' ] && [ -f '$stub/body' ] || { echo 'curl double: this case never called answer()' >&2; exit 99; }
if [ -n \"\$out\" ]; then cat '$stub/body' >\"\$out\" || { echo 'curl double: could not write the response' >&2; exit 97; }; fi
cat '$stub/status' || { echo 'curl double: could not read the status' >&2; exit 97; }"
chmod +x "$work/bin/curl" || fail "could not make the curl double executable; check that $work is not on a noexec filesystem (set \$TMPDIR elsewhere) and rerun"

# $1 = HTTP status, $2 = body (printf %b escapes). Clears any transport failure
# a prior case set.
answer() {
  rm -f "$stub/transport-fails" || fail "could not clear $stub/transport-fails; check that \$TMPDIR (or /tmp) is writable and has space, then rerun"
  put "$stub/status" '%s' "$1"
  put "$stub/body" '%b' "$2"
}

# A PATH with only what the probe needs before it reads a registry, so a case
# can take away curl or both JSON readers without taking away the shell.
mkdir -p "$work/minbin" || fail "could not create $work/minbin; check that \$TMPDIR (or /tmp) is writable and has space, then rerun"
for tool in bash dirname sed awk grep tr head mktemp rm cat; do
  path="$(tool_path "$tool")" ||
    fail "no $tool on this host, so the restricted-PATH cases cannot be built; install $tool (coreutils on Linux and macOS) and rerun"
  ln -s "$path" "$work/minbin/$tool" || fail "could not link $tool into $work/minbin; check that $work is writable"
done

stub_path="$work/bin:$PATH"

# Runs a probe as onevcs does: one argument list, the repository root as its
# working directory, only PATH and HOME. $1 = PATH, $2 = probe, then its args.
run_probe() {
  local path=$1 script=$2
  shift 2
  put "$reached" ''
  put "$work/out" ''
  put "$work/err" ''
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
    fail "$description printed output where the empty answer is promised; make the 404 branch of scripts/release-probe.sh exit 0 without printing"
  fi
}

# Every declared target: both answers, in its registry's own shape
declared="$(awk '
  /^[[:space:]]*\[/ { inside = ($0 == "[[target]]"); next }
  inside && /^id = "[^"]+"$/ { v = $0; sub(/^id = "/, "", v); sub(/"$/, "", v); print v; inside = 0 }
' release-targets.toml)" || fail "could not read release-targets.toml; restore it from git"
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
    fail "$id was requested as '$(cat "$reached")' rather than $want_url; restore that registry's url= line in scripts/release-probe.sh, or update this test's expected URL if the registry really moved"
  # The bound is curl's own, so what can be proven offline is that the flags
  # reach it: three attempts of at most 15 s, none started after 30 s, keeps
  # the probe well inside onevcs's 60 s. Proving it by timing would need a
  # double that hangs for a minute in every gate run.
  for bound in "--max-time 15" "--retry 2" "--retry-max-time 30"; do
    grep -Fq -- "$bound" "$reached" ||
      fail "$id was read without '$bound', so the probe could outlive the sixty seconds onevcs allows; restore it on the curl call in scripts/release-probe.sh"
  done
  answer 404 '{"errors":[{"detail":"Not Found"}]}'
  assert_answered "$id never released (registry 404)" "" "$stub_path" "$id"
done < <(printf '%s\n' "$declared")

# Refused before any registry is read
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
{ mkdir -p "$fixture/scripts" && cp scripts/release-probe.sh "$fixture/scripts/release-probe.sh"; } ||
  fail "could not copy the probe into $fixture; check that $work is writable"
fixture_probe="$fixture/scripts/release-probe.sh"
assert_not_answered "a checkout with no release-targets.toml" \
  "cannot read" "$stub_path" "$fixture_probe" crate:skilltest-core
put "$fixture/release-targets.toml" ''
assert_not_answered "a declaration with no schema_version" \
  "declares schema_version ''" "$stub_path" "$fixture_probe" crate:skilltest-core
for other in 2 4; do
  put "$fixture/release-targets.toml" 'schema_version = %s\n\n[[target]]\nid = "crate:skilltest-core"\n' "$other"
  assert_not_answered "a declaration at schema_version $other" \
    "declares schema_version '$other'" "$stub_path" "$fixture_probe" crate:skilltest-core
done
put "$fixture/release-targets.toml" 'schema_version = 3\n'
assert_not_answered "a declaration with no targets" \
  "declares no release targets" "$stub_path" "$fixture_probe" crate:skilltest-core
# Only a [[target]] is a release target; a retired id or a covers entry beside a
# real target is still refused.
cat >"$fixture/release-targets.toml" <<'NEIGHBOURS' || fail "could not write the neighbours fixture under $fixture"
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
put "$fixture/release-targets.toml" 'schema_version = 3\n\n[[target]]\nid = "crate:skilltest/../serde"\n'
assert_not_answered "a malformed declared crate name" \
  "is not a crate package name" "$stub_path" "$fixture_probe" "crate:skilltest/../serde"
put "$fixture/release-targets.toml" 'schema_version = 3\n\n[[target]]\nid = "pypi:skilltest sdk"\n'
assert_not_answered "a malformed declared PyPI name" \
  "is not a pypi package name" "$stub_path" "$fixture_probe" "pypi:skilltest sdk"
put "$fixture/release-targets.toml" 'schema_version = 3\n\n[[target]]\nid = "pypi:-skilltest"\n'
assert_not_answered "a declared PyPI name with a leading separator" \
  "is not a pypi package name" "$stub_path" "$fixture_probe" "pypi:-skilltest"
put "$fixture/release-targets.toml" 'schema_version = 3

[[target]]
id = "npm:@skill-test/sdk/extra"
'
assert_not_answered "a malformed declared npm name" \
  "is not a npm package name" "$stub_path" "$fixture_probe" "npm:@skill-test/sdk/extra"
put "$fixture/release-targets.toml" 'schema_version = 3\n\n[[target]]\nid = "crate:skilltest-core"\nid = "crate:skilltest-cli"\n'
assert_not_answered "a target writing id twice" \
  "writes id twice in one [[target]]" "$stub_path" "$fixture_probe" crate:skilltest-core
# A registry the probe has no URL for, even when declared.
put "$fixture/release-targets.toml" 'schema_version = 3\n\n[[target]]\nid = "gem:skilltest"\n'
assert_not_answered "a declared id on an unknown registry" \
  "unknown registry 'gem'" "$stub_path" "$fixture_probe" gem:skilltest
[ ! -s "$reached" ] ||
  fail "a fixture refusal read the network; move that refusal in scripts/release-probe.sh above the curl call, where every other declaration check sits"

# Everything a registry read can do other than answer
assert_not_answered "a host with no curl" \
  "curl is required" "$work/minbin" scripts/release-probe.sh pypi:skilltest-sdk
put "$stub/transport-fails" ''
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
for bad in 'latest' '1latest' '1..' '1' '1.2' '1.2.3rc1' '01.2.3' '1.2.3-01' '1.2.3-foo.' '1.2.3-' 'see the release notes' '1.2.3\\n' '1.2.3\\nand more'; do
  answer 200 '{"crate":{"max_stable_version":"'"$bad"'"}}'
  assert_not_answered "a response serving '$bad' as the version" \
    "version a caller can use" "$stub_path" scripts/release-probe.sh crate:skilltest-core
done
answer 200 '{"info":{"version":"0.11.2"}}'
assert_not_answered "a host with neither JSON reader" \
  "neither jq nor python3" "$work/minbin:$work/bin" scripts/release-probe.sh pypi:skilltest-sdk

# A host whose scratch space cannot be created or cleaned: the first is
# unanswered with a next action, the second leaves the answer intact and only
# warns, so a full /tmp never turns a release into "no release yet".
mkdir -p "$work/badtmp" "$work/badrm" || fail "could not create the mktemp/rm doubles' directories; check that \$TMPDIR (or /tmp) is writable and has space, then rerun"
put "$work/badtmp/mktemp" '#!/bin/sh\necho "mktemp: No space left on device" >&2\nexit 1\n'
put "$work/badrm/rm" '#!/bin/sh\n%s "$@"\necho "rm: Permission denied" >&2\nexit 1\n' "$(tool_path rm)"
chmod +x "$work/badtmp/mktemp" "$work/badrm/rm" || fail "could not make the mktemp/rm doubles executable; check that $work is not on a noexec filesystem"
answer 200 '{"info":{"version":"0.11.2"}}'
assert_not_answered "a host where mktemp fails" \
  "could not create a scratch directory" "$work/badtmp:$stub_path" scripts/release-probe.sh pypi:skilltest-sdk
assert_answered "a host where cleanup fails" "0.11.2" "$work/badrm:$stub_path" pypi:skilltest-sdk
grep -Fq "could not remove" "$work/err" ||
  fail "a failed cleanup was silent; scripts/release-probe.sh's EXIT trap must say which scratch directory to delete by hand"

# Registry specifics
# crates.io serves a prerelease as max_version while max_stable_version holds
# what a dependent may take, so the path order is load-bearing.
answer 200 '{"crate":{"max_stable_version":"0.11.2","max_version":"0.12.0-rc.1"}}'
assert_answered "a crate with a newer prerelease" "0.11.2" "$stub_path" crate:skilltest-cli
answer 200 '{"crate":{"max_stable_version":null,"max_version":"0.1.0-alpha.1"}}'
assert_answered "a crate with only a prerelease" "0.1.0-alpha.1" "$stub_path" crate:skilltest-cli

# PyPI serves a prerelease in its PEP 440 normal form, never semver's.
for good in 0.12.0rc1 1.0.post1 1.0.dev2 1!2.0 0.11.2+local.1 1; do
  answer 200 '{"info":{"version":"'"$good"'"}}'
  assert_answered "PyPI serving $good" "$good" "$stub_path" pypi:skilltest-pytest
done
for bad in 0.12.0-rc.1 0.11.2+Local 1.2.3.post; do
  answer 200 '{"info":{"version":"'"$bad"'"}}'
  assert_not_answered "PyPI serving '$bad', which is not PEP 440's normal form" \
    "version a caller can use" "$stub_path" scripts/release-probe.sh pypi:skilltest-pytest
done
answer 200 '{"dist-tags":{"latest":"0.12.0-rc.1"}}'
assert_answered "an npm prerelease" "0.12.0-rc.1" "$stub_path" npm:@skill-test/vitest

# Both readers, since either can be the one a host has — each through its
# answer, its crates.io fallback path, and every way it can fail to read one.
readers=0
for reader in jq python3; do
  path="$(tool_path "$reader")" || continue
  { mkdir -p "$work/reader-$reader" && ln -sf "$path" "$work/reader-$reader/$reader"; } ||
    fail "could not link $reader into $work/reader-$reader; check that $work is writable"
  only="$work/reader-$reader:$work/minbin:$work/bin"
  answer 200 '{"info":{"version":"0.11.2"}}'
  assert_answered "the $reader reader" "0.11.2" "$only" pypi:skilltest-sdk
  answer 200 '{"crate":{"max_stable_version":null,"max_version":"0.1.0-alpha.1"}}'
  assert_answered "the $reader reader's crates.io fallback" "0.1.0-alpha.1" "$only" crate:skilltest-core
  answer 200 'not json'
  assert_not_answered "the $reader reader on a response that is not JSON" \
    "could not parse" "$only" scripts/release-probe.sh pypi:skilltest-sdk
  answer 200 '{"info":{}}'
  assert_not_answered "the $reader reader on a response with no version" \
    "without a version at" "$only" scripts/release-probe.sh pypi:skilltest-sdk
  answer 200 '{"info":{"version":3}}'
  assert_not_answered "the $reader reader on a non-string version" \
    "without a version at" "$only" scripts/release-probe.sh pypi:skilltest-sdk
  answer 200 '{"info":"0.11.2"}'
  assert_not_answered "the $reader reader where an object is expected" \
    "without a version at" "$only" scripts/release-probe.sh pypi:skilltest-sdk
  readers=$((readers + 1))
done
[ "$readers" -gt 0 ] ||
  fail "this host has neither jq nor python3, so no reader could be exercised; install one (the probe needs it too) and rerun"

# One schema version across the declaration and both of its readers
declared_version="$(sed -n 's/^schema_version = \([0-9]*\)$/\1/p' release-targets.toml)" ||
  fail "could not read release-targets.toml; restore it from git"
for reader in scripts/release-probe.sh scripts/check-release-targets.sh; do
  reader_version="$(sed -n 's/^DECLARATION_SCHEMA_VERSION=\([0-9]*\)$/\1/p' "$reader")" ||
    fail "could not read $reader; restore it from git"
  [ -n "$reader_version" ] && [ "$reader_version" = "$declared_version" ] ||
    fail "$reader reads schema_version '$reader_version' and release-targets.toml declares '$declared_version'; bring whichever is behind up to the other"
done

echo "check-release-probe: all six targets answer a version and a 404's empty answer, and every unanswerable case is refused ($readers reader(s))"
