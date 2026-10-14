#!/usr/bin/env bash
# Test of scripts/await-registry-install.sh as publish.yml's verify-windows job
# runs it, with only `npm` and `uv` doubled on PATH and the retry interval set
# to zero: a platform package (or platform wheel) the registry lacks for the
# first attempts and then serves ends green, one that never arrives ends red
# naming it after the bound, and each invalid invocation exits 2 before any
# install. `just release-targets-check` (every `just check` tier) runs it.
#
# Quiet on success, one line. On failure it prints what the script said.
set -euo pipefail
cd "$(dirname "$0")/.." || {
  echo "await-registry-install-test: cannot enter the repository root from $0; run it from a complete checkout" >&2
  exit 1
}
script="$PWD/scripts/await-registry-install.sh"

work="$(mktemp -d)" || {
  echo "await-registry-install-test: could not create a scratch directory; check that \$TMPDIR (or /tmp) is writable and has space" >&2
  exit 1
}
trap 'rm -rf "$work" || echo "await-registry-install-test: could not remove $work; delete it by hand" >&2' EXIT

fail() {
  echo "await-registry-install-test: $1" >&2
  if [ -s "$work/out" ]; then
    echo "  what the script said:" >&2
    cat "$work/out" >&2 || echo "  (unreadable: $work/out)" >&2
  fi
  echo "  fix scripts/await-registry-install.sh (or this test, if the contract changed on purpose), then rerun just release-targets-check" >&2
  exit 1
}

# The registry doubles. Each install is counted in $CALLS. npm installs the
# SDK unless SDK_MISSING is set, and its platform package (PLATFORM) only once
# more than ARRIVES_AFTER installs ran ("never": not at all), as npm does when
# the optional dependency still 404s. uv installs the pure py3-none-any wheel
# until then, the WHEEL_TAG platform wheel after, and with UV_FAILS exits 1
# after writing it, as a reinstall that fails partway can. Each double fails
# loudly (exit 70) if it cannot record a call or write what it installs.
# Like npm, the double resolves from an existing package-lock.json, which
# records the platform package as absent when it was skipped, so a retry over
# a stale lockfile never picks it up.
# PLATFORM_MANIFEST=stale or garbled has npm write the platform package's
# manifest at another version or as broken JSON; UV_NO_METADATA has uv
# succeed without leaving dist-info behind.
mkdir -p "$work/bin" || fail "could not create $work/bin; check that $work is writable"
cat >"$work/bin/npm" <<'EOF' || fail "could not write the npm double; check that $work is writable"
#!/usr/bin/env bash
set -euo pipefail
[ "$1" = install ] || { echo "npm double: unexpected call: $*" >&2; exit 1; }
spec="${*: -1}" version="${spec##*@}"
broken() { echo "npm double: could not $1" >&2; exit 70; }
echo "$*" >>"$CALLS" || broken "log the call to $CALLS"
n="$(wc -l <"$CALLS")" || broken "count the calls in $CALLS"
[ -z "${SDK_MISSING:-}" ] || { echo "npm error 404 Not Found - GET @skill-test/sdk" >&2; exit 1; }
{ mkdir -p node_modules/@skill-test/sdk &&
  printf '{\n  "name": "@skill-test/sdk",\n  "version": "%s"\n}\n' "$version" >node_modules/@skill-test/sdk/package.json; } ||
  broken "install @skill-test/sdk into $PWD"
arrived=""
if [ "$ARRIVES_AFTER" != never ] && [ "$n" -gt "$ARRIVES_AFTER" ]; then arrived=1; fi
if [ -f package-lock.json ] && ! grep -Fq "\"node_modules/$PLATFORM\"" package-lock.json; then arrived=""; fi
if [ -n "$arrived" ]; then
  { mkdir -p "node_modules/$PLATFORM" &&
    case "${PLATFORM_MANIFEST:-}" in
      stale) printf '{"name": "%s", "version": "0.0.1"}\n' "$PLATFORM" ;;
      garbled) printf '{"name": "%s", "version": \n' "$PLATFORM" ;;
      *) printf '{\n  "name": "%s",\n  "version": "%s"\n}\n' "$PLATFORM" "$version" ;;
    esac >"node_modules/$PLATFORM/package.json"; } ||
    broken "install $PLATFORM into $PWD"
fi
if [ ! -f package-lock.json ]; then
  if [ -n "$arrived" ]; then
    printf '{"packages": {"node_modules/@skill-test/sdk": {}, "node_modules/%s": {}}}\n' "$PLATFORM"
  else
    printf '{"packages": {"node_modules/@skill-test/sdk": {}}}\n'
  fi >package-lock.json || broken "write package-lock.json into $PWD"
fi
EOF
cat >"$work/bin/uv" <<'EOF' || fail "could not write the uv double; check that $work is writable"
#!/usr/bin/env bash
set -euo pipefail
[ "$1 $2" = "pip install" ] || { echo "uv double: unexpected call: $*" >&2; exit 1; }
spec="${*: -1}" version="${spec##*==}"
broken() { echo "uv double: could not $1" >&2; exit 70; }
echo "$*" >>"$CALLS" || broken "log the call to $CALLS"
n="$(wc -l <"$CALLS")" || broken "count the calls in $CALLS"
tag=any
if [ "$ARRIVES_AFTER" != never ] && [ "$n" -gt "$ARRIVES_AFTER" ]; then tag="$WHEEL_TAG"; fi
[ -z "${UV_NO_METADATA:-}" ] || exit 0
info="$VENV/Lib/site-packages/skilltest_sdk-$version.dist-info"
{ mkdir -p "$info" &&
  printf 'Wheel-Version: 1.0\r\nGenerator: test\r\nRoot-Is-Purelib: false\r\nTag: py3-none-%s\r\n' "$tag" >"$info/WHEEL"; } ||
  broken "write $info/WHEEL"
[ -z "${UV_FAILS:-}" ] || { echo "error: Failed to install skilltest-sdk" >&2; exit 1; }
EOF
chmod +x "$work/bin/npm" "$work/bin/uv" || fail "could not make the registry doubles executable; check that $work honors chmod"

# $1 = registry, $2 = target, $3 = ARRIVES_AFTER, $4 = the declared package
# or wheel tag the double serves; rest = extra environment.
await() {
  local registry="$1" target="$2" after="$3" serves="$4" dir
  shift 4
  dir="$work/consumer-$registry"
  { rm -rf "$dir" && mkdir -p "$dir" && : >"$work/calls" && : >"$work/out"; } || fail "could not reset $work; check that it is writable"
  case "$registry" in
    npm) echo '{"name": "consumer"}' >"$dir/package.json" ;;
    *) echo 'home = /usr/bin' >"$dir/pyvenv.cfg" ;;
  esac || fail "could not mark $dir as a $registry consumer; check that $work is writable"
  case " $* " in
    *" WINDOWS_VENV=1 "*) { mkdir -p "$dir/Scripts" && : >"$dir/Scripts/python.exe"; } || fail "could not lay out $dir as a Windows venv" ;;
  esac
  env PATH="$work/bin:$PATH" CALLS="$work/calls" ARRIVES_AFTER="$after" PLATFORM="$serves" WHEEL_TAG="$serves" VENV="$dir" \
    SKILLTEST_REGISTRY_ATTEMPTS=4 SKILLTEST_REGISTRY_INTERVAL=0 "$@" \
    bash "$script" "$registry" "$target" 0.14.0 "$dir" >"$work/out" 2>&1
}
calls() { wc -l <"$work/calls" | tr -d ' '; }

# npm: the platform package 404s for two attempts, then arrives.
await npm x86_64-pc-windows-msvc 2 @skill-test/cli-win32-x64 ||
  fail "the npm install failed although the platform package arrived on the third attempt"
[ "$(calls)" = 3 ] || fail "the npm install stopped after $(calls) attempts; it must retry until the platform package is installed, then stop"
grep -Fq -- "--prefer-online @skill-test/sdk@0.14.0" "$work/calls" || fail "the npm install did not ask for @skill-test/sdk@0.14.0; got: $(head -1 "$work/calls")"
[ -f "$work/consumer-npm/node_modules/@skill-test/cli-win32-x64/package.json" ] || fail "the consumer's node_modules lacks the platform package after success"
[ "$(wc -l <"$work/out" | tr -d ' ')" = 1 ] || fail "the script printed more than one line on success; keep retries quiet until it fails"

await npm aarch64-pc-windows-msvc 0 @skill-test/cli-win32-arm64 ||
  fail "the npm install failed for aarch64-pc-windows-msvc; it must wait for release-platforms.toml's @skill-test/cli-win32-arm64"

# npm: the platform package never arrives; red after the bound, naming it.
if await npm x86_64-pc-windows-msvc never @skill-test/cli-win32-x64; then
  fail "the npm install succeeded although @skill-test/cli-win32-x64 never installed; npm skips a missing optional dependency, so the script must check node_modules"
fi
grep -Fq "@skill-test/cli-win32-x64@0.14.0 did not install from npm after 4 attempts" "$work/out" ||
  fail "the npm failure did not name @skill-test/cli-win32-x64 and the bound"
[ "$(calls)" = 4 ] || fail "the npm install made $(calls) attempts, not the 4 SKILLTEST_REGISTRY_ATTEMPTS allows"

# npm: a platform package from another target does not count.
if await npm x86_64-pc-windows-msvc 0 @skill-test/cli-win32-arm64; then
  fail "the npm install accepted another target's platform package for x86_64-pc-windows-msvc"
fi
grep -Fq "@skill-test/cli-win32-x64@0.14.0 did not install" "$work/out" || fail "the wrong-platform failure did not name @skill-test/cli-win32-x64"

# npm: a platform package left in node_modules by an earlier run does not
# count; each attempt installs into a fresh tree.
for left in node_modules package-lock.json; do
  { rm -rf "$work/consumer-npm" && mkdir -p "$work/consumer-npm/node_modules/@skill-test/cli-win32-x64" &&
    echo '{"name": "consumer"}' >"$work/consumer-npm/package.json"; } || fail "could not seed $work/consumer-npm"
  if [ "$left" = node_modules ]; then
    echo '{"name": "@skill-test/cli-win32-x64", "version": "0.14.0"}' >"$work/consumer-npm/node_modules/@skill-test/cli-win32-x64/package.json"
  else
    echo '{"packages": {"node_modules/@skill-test/sdk": {}}}' >"$work/consumer-npm/package-lock.json"
  fi || fail "could not seed a stale $left in $work/consumer-npm"
  : >"$work/calls" || fail "could not reset $work/calls"
  status=0
  env PATH="$work/bin:$PATH" CALLS="$work/calls" ARRIVES_AFTER="$([ "$left" = node_modules ] && echo never || echo 0)" \
    PLATFORM=@skill-test/cli-win32-x64 SKILLTEST_REGISTRY_ATTEMPTS=2 SKILLTEST_REGISTRY_INTERVAL=0 \
    bash "$script" npm x86_64-pc-windows-msvc 0.14.0 "$work/consumer-npm" >"$work/out" 2>&1 || status=$?
  case "$left" in
    node_modules) [ "$status" = 1 ] || fail "the npm install trusted a @skill-test/cli-win32-x64 left in node_modules by an earlier run" ;;
    *) [ "$status" = 0 ] || fail "the npm install kept a stale package-lock.json that skips @skill-test/cli-win32-x64, so the published package never installed" ;;
  esac
done

# npm: an installed platform manifest at another version, or one that is not
# JSON, is not the platform package.
for manifest in stale garbled; do
  if await npm x86_64-pc-windows-msvc 0 @skill-test/cli-win32-x64 PLATFORM_MANIFEST="$manifest"; then
    fail "the npm install accepted a $manifest @skill-test/cli-win32-x64 manifest; it must parse it and match the version"
  fi
  grep -Fq "@skill-test/cli-win32-x64@0.14.0 did not install" "$work/out" || fail "the $manifest-manifest failure did not name @skill-test/cli-win32-x64@0.14.0"
done

# npm: the SDK itself never installs.
if await npm x86_64-pc-windows-msvc 0 @skill-test/cli-win32-x64 SDK_MISSING=1; then
  fail "the npm install succeeded although @skill-test/sdk never installed"
fi
grep -Fq "@skill-test/sdk@0.14.0 did not install from npm" "$work/out" || fail "the SDK failure did not name @skill-test/sdk"

# PyPI: the pure wheel installs first, the platform wheel on the third attempt.
await pypi x86_64-pc-windows-msvc 2 win_amd64 ||
  fail "the PyPI install failed although the win_amd64 wheel arrived on the third attempt"
[ "$(calls)" = 3 ] || fail "the PyPI install stopped after $(calls) attempts; it must retry past the pure wheel until the platform wheel installed"
grep -Fq -- "--python $work/consumer-pypi/bin/python --refresh --reinstall skilltest-sdk==0.14.0" "$work/calls" || fail "the PyPI install did not reinstall skilltest-sdk==0.14.0 afresh; got: $(head -1 "$work/calls")"

await pypi aarch64-pc-windows-msvc 0 win_arm64 || fail "the PyPI install failed for aarch64-pc-windows-msvc's win_arm64 wheel"

# A Windows venv's interpreter is Scripts/python.exe, which uv must be given.
await pypi x86_64-pc-windows-msvc 0 win_amd64 WINDOWS_VENV=1 || fail "the PyPI install failed in a Windows-layout venv"
grep -Fq -- "--python $work/consumer-pypi/Scripts/python.exe " "$work/calls" ||
  fail "the PyPI install did not pass the Windows venv's Scripts/python.exe to uv; got: $(head -1 "$work/calls")"

# PyPI: only the pure wheel ever arrives; red after the bound, naming the wheel.
if await pypi x86_64-pc-windows-msvc never win_amd64; then
  fail "the PyPI install accepted the pure wheel, which bundles no CLI"
fi
grep -Fq "the py3-none-win_amd64 platform wheel of skilltest-sdk==0.14.0 (installed: py3-none-any) did not install from pypi after 4 attempts" "$work/out" ||
  fail "the PyPI failure did not name the win_amd64 platform wheel and what installed instead"

grep -Fq "the last pypi install said:" "$work/out" || fail "the PyPI failure did not show the last install's output"

# PyPI: an install that reports success but leaves no skilltest-sdk metadata
# is not one.
if await pypi x86_64-pc-windows-msvc 0 win_amd64 UV_NO_METADATA=1; then
  fail "the PyPI install succeeded although no skilltest-sdk dist-info was installed"
fi
grep -Fq "skilltest-sdk==0.14.0 did not install from pypi" "$work/out" || fail "the missing-metadata failure did not name skilltest-sdk==0.14.0"

# PyPI: an install that fails is not a success, even if it left the platform
# wheel's metadata behind.
if await pypi x86_64-pc-windows-msvc 0 win_amd64 UV_FAILS=1; then
  fail "the PyPI install succeeded although uv failed every attempt; only a successful install may be inspected"
fi
grep -Fq "skilltest-sdk==0.14.0 did not install from pypi after 4 attempts" "$work/out" ||
  fail "the failed-install refusal did not name skilltest-sdk==0.14.0"
grep -Fq "error: Failed to install skilltest-sdk" "$work/out" || fail "the failed-install refusal did not show uv's error"

# The declared-field grammars match the stager's PLATFORM_GRAMMAR, so the two
# readers of release-platforms.toml accept the same values.
stager_grammar="$(python3 -I -c '
import importlib.util, sys
spec = importlib.util.spec_from_file_location("stager", sys.argv[1])
stager = importlib.util.module_from_spec(spec)
spec.loader.exec_module(stager)
for key in ("npm_package", "wheel_tag"):
    print(key, "^" + stager.PLATFORM_GRAMMAR[key].pattern + "$")
' scripts/stage-and-verify-release-product.py)" || fail "could not read PLATFORM_GRAMMAR from scripts/stage-and-verify-release-product.py; check that python3 (3.11+) is on PATH"
while read -r key pattern; do
  grep -Fq "field=$key grammar='$pattern'" "$script" ||
    fail "scripts/await-registry-install.sh does not hold $key to the stager's grammar $pattern; make the two agree"
done <<<"$stager_grammar"

# Invocation errors exit 2, naming the problem, before any install.
# $1 = what is wrong, $2 = what the script must say, rest = its invocation.
expect_usage() {
  local what="$1" says="$2" status=0
  shift 2
  : >"$work/calls" || fail "could not reset $work/calls"
  env PATH="$work/bin:$PATH" CALLS="$work/calls" ARRIVES_AFTER=0 PLATFORM=x WHEEL_TAG=x VENV="$work" "$@" >"$work/out" 2>&1 || status=$?
  [ "$status" = 2 ] || fail "the script exited $status, not 2, where $what"
  grep -Fq -- "$says" "$work/out" || fail "the script refused where $what without saying '$says'"
  [ "$(calls)" = 0 ] || fail "the script installed before refusing where $what"
}
consumer="$work/consumer-npm"
expect_usage "the target is undeclared" "declares no npm_package for target 'x86_64-apple-darwin-nope'" \
  bash "$script" npm x86_64-apple-darwin-nope 0.14.0 "$consumer"
expect_usage "an argument is missing" "usage: await-registry-install.sh" bash "$script" npm x86_64-pc-windows-msvc 0.14.0
expect_usage "the consumer directory does not exist" "usage: await-registry-install.sh" \
  bash "$script" npm x86_64-pc-windows-msvc 0.14.0 "$work/absent"
expect_usage "the registry is unknown" "registry 'cargo' is neither npm nor pypi" \
  bash "$script" cargo x86_64-pc-windows-msvc 0.14.0 "$consumer"
expect_usage "the version is not X.Y.Z" "version 'v0.14.0' is not X.Y.Z" \
  bash "$script" npm x86_64-pc-windows-msvc v0.14.0 "$consumer"
expect_usage "the directory is not an npm consumer" "has no package.json, so it is not the npm consumer" \
  bash "$script" npm x86_64-pc-windows-msvc 0.14.0 "$work/bin"
expect_usage "the directory is the filesystem root" "has no package.json" \
  bash "$script" npm x86_64-pc-windows-msvc 0.14.0 /
expect_usage "the attempt bound is not a positive number" "unset them to use the defaults" \
  SKILLTEST_REGISTRY_ATTEMPTS=0 bash "$script" npm x86_64-pc-windows-msvc 0.14.0 "$consumer"
expect_usage "the interval is not a number" "unset them to use the defaults" \
  SKILLTEST_REGISTRY_INTERVAL=soon bash "$script" npm x86_64-pc-windows-msvc 0.14.0 "$consumer"

# A declared npm_package that could leave node_modules is refused before any
# install: the script runs from a scratch tree holding that declaration.
{ mkdir -p "$work/tree/scripts" && cp scripts/await-registry-install.sh "$work/tree/scripts/" &&
  sed 's|"@skill-test/cli-win32-x64"|"../../escape"|' release-platforms.toml >"$work/tree/release-platforms.toml"; } ||
  fail "could not build the scratch declaration tree; check that $work is writable"
: >"$work/calls" || fail "could not reset $work/calls"
if env PATH="$work/bin:$PATH" CALLS="$work/calls" bash "$work/tree/scripts/await-registry-install.sh" npm x86_64-pc-windows-msvc 0.14.0 "$consumer" >"$work/out" 2>&1; then
  fail "the script accepted a declared npm_package that is not a package name"
fi
grep -Fq "declares npm_package '../../escape' for x86_64-pc-windows-msvc, which is not a valid npm_package" "$work/out" ||
  fail "the script refused a malformed npm_package without naming it"
[ "$(calls)" = 0 ] || fail "the script installed before refusing a malformed npm_package"

echo "await-registry-install-test: npm and PyPI installs wait for the target's platform package and wheel, and name the one that never arrives"
