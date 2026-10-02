#!/usr/bin/env bash
# Behavioral test of scripts/check-release-targets.sh, the release-target drift
# gate. A gate nobody has watched fail is not known to work, and what this one
# guards against is an inventory going stale in silence — so it is driven against
# a staged copy of everything it reads, once per way the declaration and
# publish.yml can drift apart in each direction, and must go red naming the
# drift. It must also be green on the unmodified copy, or every red is noise.
#
# Quiet on success, one line. On failure it prints what the gate said.
set -euo pipefail
cd "$(dirname "$0")/.."

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

fail() {
  echo "check-release-targets-test: $1" >&2
  [ -s "$work/out" ] && { echo "  what the gate said:" >&2; cat "$work/out" >&2; }
  exit 1
}

# Everything the gate reads.
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
  rm -rf "$work/repo"
  local f
  for f in "${staged[@]}"; do
    mkdir -p "$work/repo/$(dirname "$f")"
    cp -p "$f" "$work/repo/$f"
  done
}

# Portable in-place edit (BSD and GNU sed disagree on -i). $1 = file, rest = sed args.
edit() {
  local file="$work/repo/$1"
  shift
  sed "$@" "$file" >"$file.new" && mv "$file.new" "$file"
}

run_gate() {
  local status=0
  bash "$work/repo/scripts/check-release-targets.sh" >"$work/out" 2>&1 || status=$?
  return "$status"
}

# $1 = what drifted, $2 = what the gate must say about it. Stage, then the
# caller's mutation runs, then this asserts red with that diagnosis.
expect_red() {
  if run_gate; then
    fail "the gate stayed green when $1; it must refuse that drift"
  fi
  grep -Fq -- "$2" "$work/out" ||
    fail "the gate went red when $1, but not for that reason (expected it to say '$2'), so the case exercised some other branch"
}

stage
run_gate || fail "the gate is red on an unmodified copy of this tree, so no red below would mean anything"

# Published but not declared: drop the pytest plugin's [[target]].
stage
# Paragraph mode: each [[target]] is one blank-line-separated block.
awk 'BEGIN { RS = ""; ORS = "\n\n" } !/id = "pypi:skilltest-pytest"/' \
  "$work/repo/release-targets.toml" >"$work/decl"
mv "$work/decl" "$work/repo/release-targets.toml"
expect_red "a published PyPI project lost its target" \
  "publishes 'pypi:skilltest-pytest' (from plugins/pytest/pyproject.toml) and release-targets.toml declares no target"

# Published but not declared: a new crate starts publishing.
stage
mkdir -p "$work/repo/crates/skilltest-extra"
printf '[package]\nname = "skilltest-extra"\n' >"$work/repo/crates/skilltest-extra/Cargo.toml"
edit .github/workflows/publish.yml 's/^\( *\)publish_crate skilltest-cli$/&\
\1publish_crate skilltest-extra/'
expect_red "publish.yml started publishing an undeclared crate" \
  "publishes 'crate:skilltest-extra'"

# Declared but not published: publish.yml stops publishing the vitest plugin.
stage
edit .github/workflows/publish.yml '/^ *publish_pkg plugins\/vitest/d'
expect_red "a declared npm package stopped being published" \
  "declares 'npm:@skill-test/vitest', which .github/workflows/publish.yml does not publish"

# Declared but not published: a renamed manifest.
stage
edit sdks/python/pyproject.toml 's/^name = "skilltest-sdk"$/name = "skilltest-sdk-renamed"/'
expect_red "the Python SDK's manifest was renamed" \
  "declares 'pypi:skilltest-sdk', which .github/workflows/publish.yml does not publish"

# Published but not covered: a covers entry dropped.
stage
edit release-targets.toml '/"npm:@skill-test\/cli-darwin-x64",/d'
expect_red "a published platform package lost its covers entry" \
  "publishes per-platform package 'npm:@skill-test/cli-darwin-x64'"

# Covered but not published: a platform publish.yml never stages.
stage
edit release-targets.toml 's/^  "npm:@skill-test\/cli-darwin-arm64",$/&\
  "npm:@skill-test\/cli-win32-x64",/'
expect_red "a covers entry names a package nothing publishes" \
  "cover 'npm:@skill-test/cli-win32-x64', which .github/workflows/publish.yml does not publish"

# A committed platform package the npm job's loop never stages.
stage
edit .github/workflows/publish.yml 's/ x86_64-apple-darwin aarch64-apple-darwin; do/ aarch64-apple-darwin; do/'
expect_red "the npm job stopped staging a committed platform package" \
  "sdks/typescript/platforms/cli-darwin-x64/package.json is committed but"

# Covered but not pinned by the covering target's optionalDependencies.
stage
jq 'del(.optionalDependencies["@skill-test/cli-linux-arm64"])' "$work/repo/sdks/typescript/package.json" \
  >"$work/pkg" && mv "$work/pkg" "$work/repo/sdks/typescript/package.json"
expect_red "the SDK stopped pinning a covered platform package" \
  "does not pin it in optionalDependencies"

# A declared target pointing at the wrong manifest.
stage
edit release-targets.toml 's|^manifest = "plugins/vitest/package.json"$|manifest = "sdks/typescript/package.json"|'
expect_red "a target named the wrong manifest" \
  "gives 'npm:@skill-test/vitest' manifest \"sdks/typescript/package.json\""

# Targets out of publication order.
stage
# Swap the first two blocks that open a target (the two crates).
awk 'BEGIN { RS = ""; ORS = "\n\n" }
  /\[\[target\]\]/ && ++n == 1 { held = $0; next }
  { print }
  n == 2 && held != "" { print held; held = "" }
' "$work/repo/release-targets.toml" >"$work/decl"
mv "$work/decl" "$work/repo/release-targets.toml"
expect_red "the targets were listed out of publication order" \
  "in a different order than"

# A key the schema does not declare, which must not read as an absent field.
stage
edit release-targets.toml 's/^manifest = "crates\/skilltest-core\/Cargo.toml"$/manifset = "crates\/skilltest-core\/Cargo.toml"/'
expect_red "a key was misspelled" 'names "manifset"'

# A schema_version this gate does not read.
stage
edit release-targets.toml 's/^schema_version = 3$/schema_version = 2/'
expect_red "the declaration moved off schema_version 3" "declares schema_version '2'"

echo "check-release-targets-test: the drift gate is green on this tree and red on each direction of drift"
