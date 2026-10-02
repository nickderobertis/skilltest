#!/usr/bin/env bash
# llmlint: ignore-file[new_code_lands_in_a_project] tests scripts/check-release-targets.sh, repo-level release glue that belongs to no Nx package; run workspace-wide from `just check` (AGENTS.md: scripts/*.sh are orchestrator-independent glue).
# Behavioral test of scripts/check-release-targets.sh, the release-target drift
# gate. A gate nobody has watched fail is not known to work, and what this one
# guards against is an inventory going stale in silence — so it is driven against
# a staged copy of everything it reads, once per way the declaration and
# publish.yml can drift apart in each direction, and must go red naming the
# drift. It must also be green on the unmodified copy, or every red is noise.
#
# Quiet on success, one line. On failure it prints what the gate said.
set -euo pipefail
cd "$(dirname "$0")/.." || {
  echo "check-release-targets-test: cannot enter the repository root from $0; run it from a complete checkout" >&2
  exit 1
}

work="$(mktemp -d)" || {
  echo "check-release-targets-test: could not create a scratch directory; check that \$TMPDIR (or /tmp) is writable and has space" >&2
  exit 1
}
trap 'rm -rf "$work" || echo "check-release-targets-test: could not remove $work; delete it by hand" >&2' EXIT

fail() {
  echo "check-release-targets-test: $1" >&2
  [ -s "$work/out" ] && { echo "  what the gate said:" >&2; cat "$work/out" >&2; }
  exit 1
}

staged=(
  release-targets.toml
  .github/workflows/publish.yml
  scripts/check-release-targets.sh
  scripts/release-probe.sh
  scripts/stage-npm-binary.sh
  crates/skilltest-core/Cargo.toml
  crates/skilltest-cli/Cargo.toml
  sdks/python/pyproject.toml
  plugins/pytest/pyproject.toml
  sdks/typescript/package.json
  plugins/vitest/package.json
  sdks/typescript/platforms/cli-linux-x64/package.json
  sdks/typescript/platforms/cli-linux-arm64/package.json
  sdks/typescript/platforms/cli-darwin-x64/package.json
  sdks/typescript/platforms/cli-darwin-arm64/package.json
)

stage() {
  rm -rf "$work/repo" || fail "could not clear $work/repo between cases; check its permissions"
  local f
  for f in "${staged[@]}"; do
    { mkdir -p "$work/repo/$(dirname "$f")" && cp -p "$f" "$work/repo/$f"; } ||
      fail "could not stage $f into $work/repo; restore $f from git or free space in \$TMPDIR"
  done
}

# Replaces a staged file with what was written to $work/next; a failure here is
# the harness's, not the gate's, so it is reported as such.
replace() {
  mv "$work/next" "$work/repo/$1" || fail "could not rewrite the staged $1; check that $work is writable"
}

# Portable in-place edit (BSD and GNU sed disagree on -i). $1 = file, rest = sed args.
edit() {
  local rel=$1
  shift
  sed "$@" "$work/repo/$rel" >"$work/next" || fail "could not apply this case's sed edit to the staged $rel; fix the expression in this test"
  replace "$rel"
}

run_gate() {
  : >"$work/out" || fail "could not write $work/out to capture the gate; check that $work is writable"
  bash "$work/repo/scripts/check-release-targets.sh" >"$work/out" 2>&1
}

# $1 = what drifted, $2 = what the gate must say about it. Stage, then the
# caller's mutation runs, then this asserts red with that diagnosis.
expect_red() {
  if run_gate; then
    fail "the gate stayed green when $1; restore the comparison in scripts/check-release-targets.sh that refuses it"
  fi
  grep -Fq -- "$2" "$work/out" ||
    fail "the gate went red when $1, but not for that reason (expected it to say '$2'); fix the branch of scripts/check-release-targets.sh that now reports it, or this case's mutation if it no longer produces that drift"
}

stage
run_gate || fail "the gate is red on an unmodified copy of this tree, so no red below would mean anything; run 'bash scripts/check-release-targets.sh' and fix what it reports first"

stage
awk 'BEGIN { RS = ""; ORS = "\n\n" } !/id = "pypi:skilltest-pytest"/' \
  "$work/repo/release-targets.toml" >"$work/next" || fail "could not drop the pytest target from the staged declaration"
replace release-targets.toml
expect_red "a published PyPI project lost its target" \
  "publishes 'pypi:skilltest-pytest' (from plugins/pytest/pyproject.toml) and release-targets.toml declares no target"

stage
{ mkdir -p "$work/repo/crates/skilltest-extra" &&
  printf '[package]\nname = "skilltest-extra"\n' >"$work/repo/crates/skilltest-extra/Cargo.toml"; } ||
  fail "could not write the extra crate's staged manifest; check that $work is writable"
edit .github/workflows/publish.yml 's/^\( *\)publish_crate skilltest-cli$/&\
\1publish_crate skilltest-extra/'
expect_red "publish.yml started publishing an undeclared crate" \
  "publishes 'crate:skilltest-extra'"

stage
edit .github/workflows/publish.yml '/^ *publish_pkg plugins\/vitest/d'
expect_red "a declared npm package stopped being published" \
  "declares 'npm:@skill-test/vitest', which .github/workflows/publish.yml does not publish"

stage
edit sdks/python/pyproject.toml 's/^name = "skilltest-sdk"$/name = "skilltest-sdk-renamed"/'
expect_red "the Python SDK's manifest was renamed" \
  "declares 'pypi:skilltest-sdk', which .github/workflows/publish.yml does not publish"

stage
edit release-targets.toml '/"npm:@skill-test\/cli-darwin-x64",/d'
expect_red "a published platform package lost its covers entry" \
  "publishes per-platform package 'npm:@skill-test/cli-darwin-x64'"

stage
edit release-targets.toml 's/^  "npm:@skill-test\/cli-darwin-arm64",$/&\
  "npm:@skill-test\/cli-win32-x64",/'
expect_red "a covers entry names a package nothing publishes" \
  "cover 'npm:@skill-test/cli-win32-x64', which .github/workflows/publish.yml does not publish"

stage
edit .github/workflows/publish.yml 's/ x86_64-apple-darwin aarch64-apple-darwin; do/ aarch64-apple-darwin; do/'
expect_red "the npm job stopped staging a committed platform package" \
  "sdks/typescript/platforms/cli-darwin-x64/package.json is committed but"

stage
jq 'del(.optionalDependencies["@skill-test/cli-linux-arm64"])' "$work/repo/sdks/typescript/package.json" \
  >"$work/next" || fail "could not drop the arm64 pin from the staged SDK manifest"
replace sdks/typescript/package.json
expect_red "the SDK stopped pinning a covered platform package" \
  "does not pin it in optionalDependencies"

stage
edit release-targets.toml 's|^manifest = "plugins/vitest/package.json"$|manifest = "sdks/typescript/package.json"|'
expect_red "a target named the wrong manifest" \
  "gives 'npm:@skill-test/vitest' manifest \"sdks/typescript/package.json\""

stage
awk 'BEGIN { RS = ""; ORS = "\n\n" }
  /\[\[target\]\]/ && ++n == 1 { held = $0; next }
  { print }
  n == 2 && held != "" { print held; held = "" }
' "$work/repo/release-targets.toml" >"$work/next" || fail "could not swap the two crate targets in the staged declaration"
replace release-targets.toml
expect_red "the targets were listed out of publication order" \
  "in a different order than"

stage
edit release-targets.toml 's/^manifest = "crates\/skilltest-core\/Cargo.toml"$/manifset = "crates\/skilltest-core\/Cargo.toml"/'
expect_red "the manifest key was misspelled" "lacks an id, name or manifest"

stage
edit release-targets.toml 's/^name = "core"$/&\
name = "core-again"/'
expect_red "a target wrote its short name twice" "writes name twice in one entry"

stage
edit release-targets.toml 's/^manifest = "plugins\/vitest\/package.json"$/manifest = ["plugins\/vitest\/package.json"]/'
expect_red "a manifest was written as a list" "writes manifest as something other than one non-empty quoted string"

stage
jq '.optionalDependencies["@skill-test/cli-linux-x64"] = 1' "$work/repo/sdks/typescript/package.json" \
  >"$work/next" || fail "could not rewrite the staged SDK manifest's x64 pin"
replace sdks/typescript/package.json
expect_red "the SDK pinned a platform package by a non-string spec" "is pinned by a non-string spec"

stage
edit release-targets.toml 's/^  "npm:@skill-test\/cli-linux-x64",$/  "npm:@skill-test\/cli-linux-x64"/'
expect_red "a multi-line covers list lost a comma" "with no comma before the next one"

stage
edit release-targets.toml 's/^schema_version = 3$/schema_version = 2/'
expect_red "the declaration moved off schema_version 3" "declares schema_version '2'"

echo "check-release-targets-test: the drift gate is green on this tree and red on each direction of drift"
