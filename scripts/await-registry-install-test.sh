#!/usr/bin/env bash
# Test of scripts/await-registry-install.sh as publish.yml's verify-windows job
# runs it, with only `npm` and `uv` doubled on PATH and the retry interval set
# to zero: a platform package (or platform wheel) the registry lacks for the
# first attempts and then serves ends green, one that never arrives ends red
# naming it after the bound, and an undeclared target is refused.
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
  exit 1
}

# The registry doubles. Each install is counted in $CALLS. npm installs the
# SDK unless SDK_MISSING is set, and its platform package (PLATFORM) only once
# more than ARRIVES_AFTER installs ran ("never": not at all), as npm does when
# the optional dependency still 404s. uv installs the pure py3-none-any wheel
# until then, the WHEEL_TAG platform wheel after.
mkdir -p "$work/bin" || fail "could not create $work/bin; check that $work is writable"
cat >"$work/bin/npm" <<'EOF' || fail "could not write the npm double; check that $work is writable"
#!/usr/bin/env bash
[ "$1" = install ] || { echo "npm double: unexpected call: $*" >&2; exit 1; }
spec="${*: -1}" version="${spec##*@}"
echo "$*" >>"$CALLS"
n="$(wc -l <"$CALLS")"
[ -z "${SDK_MISSING:-}" ] || { echo "npm error 404 Not Found - GET @skill-test/sdk" >&2; exit 1; }
mkdir -p node_modules/@skill-test/sdk
printf '{\n  "name": "@skill-test/sdk",\n  "version": "%s"\n}\n' "$version" >node_modules/@skill-test/sdk/package.json
if [ "$ARRIVES_AFTER" != never ] && [ "$n" -gt "$ARRIVES_AFTER" ]; then
  mkdir -p "node_modules/$PLATFORM"
  printf '{\n  "name": "%s",\n  "version": "%s"\n}\n' "$PLATFORM" "$version" >"node_modules/$PLATFORM/package.json"
fi
EOF
cat >"$work/bin/uv" <<'EOF' || fail "could not write the uv double; check that $work is writable"
#!/usr/bin/env bash
[ "$1 $2" = "pip install" ] || { echo "uv double: unexpected call: $*" >&2; exit 1; }
spec="${*: -1}" version="${spec##*==}"
echo "$*" >>"$CALLS"
n="$(wc -l <"$CALLS")"
tag=any
if [ "$ARRIVES_AFTER" != never ] && [ "$n" -gt "$ARRIVES_AFTER" ]; then tag="$WHEEL_TAG"; fi
info="$VENV/Lib/site-packages/skilltest_sdk-$version.dist-info"
mkdir -p "$info"
printf 'Wheel-Version: 1.0\r\nGenerator: test\r\nRoot-Is-Purelib: false\r\nTag: py3-none-%s\r\n' "$tag" >"$info/WHEEL"
EOF
chmod +x "$work/bin/npm" "$work/bin/uv" || fail "could not make the registry doubles executable; check that $work honors chmod"

# $1 = registry, $2 = target, $3 = ARRIVES_AFTER, $4 = the declared package
# or wheel tag the double serves; rest = extra environment.
await() {
  local registry="$1" target="$2" after="$3" serves="$4" dir
  shift 4
  dir="$work/consumer-$registry"
  { rm -rf "$dir" && mkdir -p "$dir" && : >"$work/calls" && : >"$work/out"; } || fail "could not reset $work; check that it is writable"
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

# npm: the SDK itself never installs.
if await npm x86_64-pc-windows-msvc 0 @skill-test/cli-win32-x64 SDK_MISSING=1; then
  fail "the npm install succeeded although @skill-test/sdk never installed"
fi
grep -Fq "@skill-test/sdk@0.14.0 did not install from npm" "$work/out" || fail "the SDK failure did not name @skill-test/sdk"

# PyPI: the pure wheel installs first, the platform wheel on the third attempt.
await pypi x86_64-pc-windows-msvc 2 win_amd64 ||
  fail "the PyPI install failed although the win_amd64 wheel arrived on the third attempt"
[ "$(calls)" = 3 ] || fail "the PyPI install stopped after $(calls) attempts; it must retry past the pure wheel until the platform wheel installed"
grep -Fq -- "--refresh --reinstall skilltest-sdk==0.14.0" "$work/calls" || fail "the PyPI install did not reinstall skilltest-sdk==0.14.0 afresh; got: $(head -1 "$work/calls")"

await pypi aarch64-pc-windows-msvc 0 win_arm64 || fail "the PyPI install failed for aarch64-pc-windows-msvc's win_arm64 wheel"

# PyPI: only the pure wheel ever arrives; red after the bound, naming the wheel.
if await pypi x86_64-pc-windows-msvc never win_amd64; then
  fail "the PyPI install accepted the pure wheel, which bundles no CLI"
fi
grep -Fq "the py3-none-win_amd64 platform wheel of skilltest-sdk==0.14.0 (installed: py3-none-any) did not install from pypi after 4 attempts" "$work/out" ||
  fail "the PyPI failure did not name the win_amd64 platform wheel and what installed instead"

# Invocation errors exit 2 before any install.
await npm x86_64-apple-darwin-nope 0 x && fail "the script accepted an undeclared target"
grep -Fq "declares no npm_package for target 'x86_64-apple-darwin-nope'" "$work/out" || fail "the undeclared target was refused without naming it"
[ "$(calls)" = 0 ] || fail "the script installed before refusing an undeclared target"

echo "await-registry-install-test: npm and PyPI installs wait for the target's platform package and wheel, and name the one that never arrives"
