#!/usr/bin/env bash
# llmlint: ignore-file[new_code_lands_in_a_project] tests scripts/check-release-targets.sh, repo-level release glue that belongs to no Nx package; run workspace-wide from `just check` (AGENTS.md: scripts/*.sh are orchestrator-independent glue).
# Test of scripts/check-release-targets.sh on a staged copy of what it reads: it
# must be green unmodified, or no red means anything, and red naming the drift
# on each way the declaration and publish.yml can disagree, since a gate nobody
# has watched fail is not known to work.
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
  if [ -s "$work/out" ]; then
    echo "  what the gate said:" >&2
    cat "$work/out" >&2 || echo "  (unreadable: $work/out)" >&2
  fi
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
  sdks/typescript/platforms/cli-win32-x64/package.json
  sdks/typescript/platforms/cli-win32-arm64/package.json
  release-platforms.toml
  .github/workflows/release.yml
  .github/workflows/windows-build.yml
  .github/workflows/bundle-smoke.yml
  scripts/build-python-dist.sh
  scripts/build-python-wheel.sh
  scripts/set-version.sh
  .releaserc.json
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
  "$work/repo/release-targets.toml" >"$work/next" || fail "could not drop the pytest target from the staged declaration; check that $work is writable and has space, then rerun"
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
  "npm:@skill-test\/cli-freebsd-x64",/'
expect_red "a covers entry names a package nothing publishes" \
  "cover 'npm:@skill-test/cli-freebsd-x64', which .github/workflows/publish.yml does not publish"

stage
edit .github/workflows/publish.yml 's/ x86_64-apple-darwin aarch64-apple-darwin \\$/ aarch64-apple-darwin \\/'
expect_red "the npm job stopped staging a committed platform package" \
  "sdks/typescript/platforms/cli-darwin-x64/package.json is committed but"

stage
jq 'del(.optionalDependencies["@skill-test/cli-linux-arm64"])' "$work/repo/sdks/typescript/package.json" \
  >"$work/next" || fail "could not drop the arm64 pin from the staged SDK manifest; read jq's error above — fix sdks/typescript/package.json if it is not JSON, else $work's permissions or space"
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
' "$work/repo/release-targets.toml" >"$work/next" || fail "could not swap the two crate targets in the staged declaration; check that $work is writable and has space, then rerun"
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
  >"$work/next" || fail "could not rewrite the staged SDK manifest's x64 pin; read jq's error above — fix sdks/typescript/package.json if it is not JSON, else $work's permissions or space"
replace sdks/typescript/package.json
expect_red "the SDK pinned a platform package by a non-string spec" "is pinned by a non-string spec"

stage
jq '.optionalDependencies["@skill-test/cli-win32-arm64"] = "*"' "$work/repo/sdks/typescript/package.json" \
  >"$work/next" || fail "could not rewrite the staged SDK manifest's win32-arm64 pin; read jq's error above — fix sdks/typescript/package.json if it is not JSON, else $work's permissions or space"
replace sdks/typescript/package.json
expect_red "the SDK pinned a platform package to any release" "@skill-test/cli-win32-arm64 is pinned by *, not workspace:*"

stage
edit release-targets.toml 's/^  "npm:@skill-test\/cli-linux-x64",$/  "npm:@skill-test\/cli-linux-x64"/'
expect_red "a multi-line covers list lost a comma" "with no comma before the next one"

stage
printf '[package]\nname = "skilltest-cli"\nversion = \n' >"$work/repo/crates/skilltest-cli/Cargo.toml" ||
  fail "could not write the truncated crate manifest; check that $work is writable and has space, then rerun"
expect_red "a crate manifest is not valid TOML" "publishes crate 'skilltest-cli' but no crates/*/Cargo.toml has that [package] name"

stage
printf '{"name": "@skill-test/cli-linux-x64"}\n{"name": "@skill-test/other"\n' \
  >"$work/repo/sdks/typescript/platforms/cli-linux-x64/package.json" ||
  fail "could not write the truncated platform manifest; check that $work is writable and has space, then rerun"
expect_red "a platform manifest parsed only partway" 'is not JSON with a string "name"'

stage
edit .github/workflows/publish.yml '/^ *- { target: aarch64-pc-windows-msvc, /d'
expect_red "publish.yml's binaries matrix lost a Windows target" \
  "publish.yml's binaries matrix lacks 'aarch64-pc-windows-msvc	windows-11-arm	skilltest.exe'"

stage
edit .github/workflows/release.yml 's/{ target: x86_64-pc-windows-msvc, os: windows-latest, bin: skilltest.exe }/{ target: x86_64-pc-windows-msvc, os: windows-latest, bin: skilltest }/'
expect_red "release.yml's archive matrix named the Windows binary without .exe" \
  "release.yml's upload matrix has 'x86_64-pc-windows-msvc	windows-latest	skilltest'"

stage
edit scripts/build-python-dist.sh 's/ x86_64-pc-windows-msvc aarch64-pc-windows-msvc"$/"/'
expect_red "build-python-dist.sh stopped building the Windows wheels" \
  "build-python-dist.sh's targets lacks 'x86_64-pc-windows-msvc'"

stage
edit scripts/build-python-wheel.sh '/^aarch64-pc-windows-msvc) plat=/d'
expect_red "build-python-wheel.sh lost the win_arm64 tag" \
  "build-python-wheel.sh's tag map lacks 'aarch64-pc-windows-msvc	win_arm64'"

stage
edit .github/workflows/windows-build.yml '/^ *- { target: aarch64-pc-windows-msvc, /d'
expect_red "the Windows PR lane stopped building a Windows target" \
  "windows-build.yml's matrix lacks 'aarch64-pc-windows-msvc	windows-11-arm	skilltest.exe'"

stage
awk '/^  verify-windows:$/ { inside = 1 } !(inside && /- { target: x86_64-pc-windows-msvc, /)' \
  "$work/repo/.github/workflows/publish.yml" >"$work/next" || fail "could not drop the x64 row from the staged install proof; check that $work is writable and has space, then rerun"
replace .github/workflows/publish.yml
expect_red "the release-time install proof stopped proving a Windows target" \
  "publish.yml's verify-windows matrix lacks 'x86_64-pc-windows-msvc	windows-latest	skilltest.exe'"

stage
edit .github/workflows/windows-build.yml 's/^\( *\)- { target: x86_64-pc-windows-msvc, .*$/&\
\1- { target: x86_64-unknown-linux-gnu, os: ubuntu-latest, bin: skilltest }/'
expect_red "the Windows PR lane built a non-Windows target" \
  "windows-build.yml's matrix has 'x86_64-unknown-linux-gnu	ubuntu-latest	skilltest'"

stage
awk 'BEGIN { RS = ""; ORS = "\n\n" } !/target = "aarch64-pc-windows-msvc"/' \
  "$work/repo/release-platforms.toml" >"$work/next" || fail "could not drop the win_arm64 platform from the staged declaration; check that $work is writable and has space, then rerun"
replace release-platforms.toml
expect_red "a shipped Windows target was dropped from the platform declaration" \
  "publish.yml's binaries matrix has 'aarch64-pc-windows-msvc	windows-11-arm	skilltest.exe', which release-platforms.toml does not declare"

stage
edit .github/workflows/bundle-smoke.yml 's/^\( *\)- { target: aarch64-apple-darwin, os: macos-14 }$/&\
\1- { target: x86_64-unknown-freebsd, os: ubuntu-latest }/'
expect_red "bundle-smoke smoked an undeclared platform" \
  "bundle-smoke.yml smokes 'x86_64-unknown-freebsd', which release-platforms.toml does not declare"

stage
edit .github/workflows/bundle-smoke.yml 's/{ target: aarch64-apple-darwin, os: macos-14 }/{ target: aarch64-apple-darwin, os: macos-13 }/'
expect_red "bundle-smoke smoked a declared platform on another runner" \
  "smokes 'aarch64-apple-darwin' on 'macos-13' but release-platforms.toml builds it on 'macos-14'"

stage
edit .github/workflows/publish.yml 's/^\( *\)x86_64-pc-windows-msvc aarch64-pc-windows-msvc; do$/\1x86_64-pc-windows-msvc; do/'
expect_red "the npm job's loop lost a Windows target" \
  "npm 'for target in' loop lacks 'aarch64-pc-windows-msvc'"

stage
jq 'del(.optionalDependencies["@skill-test/cli-win32-x64"])' "$work/repo/sdks/typescript/package.json" \
  >"$work/next" || fail "could not drop the win32-x64 pin from the staged SDK manifest; read jq's error above — fix sdks/typescript/package.json if it is not JSON, else $work's permissions or space"
replace sdks/typescript/package.json
expect_red "the SDK's optionalDependencies lost a Windows platform package" \
  "optionalDependencies lacks '@skill-test/cli-win32-x64'"

stage
edit .releaserc.json '/"sdks\/typescript\/platforms\/cli-win32-arm64\/package.json",/d'
expect_red ".releaserc.json stopped committing a Windows platform package's version" \
  "@semantic-release/git assets lacks 'cli-win32-arm64'"

stage
printf '{"plugins": [\n' >"$work/repo/.releaserc.json" ||
  fail "could not write the truncated release config; check that $work is writable and has space, then rerun"
expect_red ".releaserc.json stopped being readable JSON" \
  ".releaserc.json's @semantic-release/git assets cannot be read"

stage
edit scripts/stage-npm-binary.sh 's/^x86_64-pc-windows-msvc) pkg="cli-win32-x64" ;;$/x86_64-pc-windows-msvc) pkg="cli-win32-arm64" ;;/'
expect_red "the stager mapped a Windows target to the wrong package" \
  "stage-npm-binary.sh's package map has 'x86_64-pc-windows-msvc	cli-win32-arm64'"

stage
edit release-targets.toml '/"npm:@skill-test\/cli-win32-arm64",/d'
expect_red "the covers lost a Windows platform package" \
  "npm:@skill-test/sdk covers lacks '@skill-test/cli-win32-arm64'"

stage
edit release-platforms.toml 's/^npm_package = "@skill-test\/cli-win32-x64"$/npm_package = "@skill-test\/cli-windows-x64"/'
expect_red "the declaration renamed a platform package the npm side still publishes" \
  "sdks/typescript/platforms/ has 'cli-win32-x64	@skill-test/cli-win32-x64', which release-platforms.toml does not declare"

stage
edit scripts/set-version.sh 's|^for pkg in sdks/typescript/platforms/\*/package.json; do$|for pkg in sdks/typescript/platforms/cli-linux-*/package.json; do|'
expect_red "set-version.sh stopped versioning every platform package" \
  "no longer versions every platform package"

stage
edit release-platforms.toml '/^wheel_tag = "win_amd64"$/d'
expect_red "a declared platform lost a field" "[[platform]] 5 lacks one of"

stage
edit release-targets.toml 's/^schema_version = 3$/schema_version = 2/'
expect_red "the declaration moved off schema_version 3" "declares schema_version '2'"

stage
printf '[[platform]\n' >>"$work/repo/release-platforms.toml" || fail "could not append to the staged release-platforms.toml; check that $work is writable"
expect_red "the platform declaration is not valid TOML" "release-platforms.toml is not readable TOML"

stage
edit release-platforms.toml "/^\[\[platform\]\]\$/,\$d"
expect_red "the platform declaration lost every [[platform]]" "release-platforms.toml declares no [[platform]]"

stage
edit release-platforms.toml "/^\[\[platform\]\]\$/,\$d"
printf 'platform = "x86_64-unknown-linux-gnu"\n' >>"$work/repo/release-platforms.toml" || fail "could not append to the staged release-platforms.toml; check that $work is writable"
expect_red "the platform list was written as a string" "release-platforms.toml declares no [[platform]]"

stage
edit release-platforms.toml 's/^bin = "skilltest.exe"$/bin = 3/'
expect_red "a platform field was not a string" "[[platform]] 5 lacks one of"

stage
edit release-platforms.toml 's/^target = "aarch64-unknown-linux-gnu"$/target = "x86_64-unknown-linux-gnu"/'
expect_red "two platforms declared one target" \
  "release-platforms.toml declares 'x86_64-unknown-linux-gnu' on more than one [[platform]]"

stage
edit release-platforms.toml 's/^npm_package = "@skill-test\/cli-linux-arm64"$/npm_package = "@skill-test\/cli-linux-x64"/'
expect_red "two platforms declared one npm package" \
  "release-platforms.toml declares '@skill-test/cli-linux-x64' on more than one [[platform]]"

stage
edit release-platforms.toml 's/^npm_dir = "cli-linux-arm64"$/npm_dir = "cli-linux-x64"/'
expect_red "two platforms declared one package directory" \
  "release-platforms.toml declares 'cli-linux-x64' on more than one [[platform]]"

stage
edit scripts/build-python-dist.sh 's/ aarch64-pc-windows-msvc"$/ aarch64-pc-windows-msvc aarch64-pc-windows-msvc"/'
expect_red "a restated platform list repeated an entry" \
  "build-python-dist.sh's targets lists 'aarch64-pc-windows-msvc' more than once"

echo "check-release-targets-test: the drift gate is green on this tree and red on each direction of drift, platform lists included"
