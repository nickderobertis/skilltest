#!/usr/bin/env bash
# Installs a just-published SDK from its registry the way a consumer does, and
# retries until what installed is the target's bundled CLI, not merely the SDK.
# publish.yml's verify-windows job runs it before running the bundled CLI.
#
# Usage: await-registry-install.sh npm <target> <version> <consumer-dir>
#        await-registry-install.sh pypi <target> <version> <venv-dir>
#   npm  installs @skill-test/sdk@<version> into <consumer-dir> (an npm
#        project) until it and the target's npm_package from
#        release-platforms.toml are both in its node_modules: npm skips an
#        optional dependency the registry still 404s on without failing.
#   pypi installs skilltest-sdk==<version> into the venv at <venv-dir> until
#        the installed wheel is the target's platform wheel (Tag:
#        py3-none-<wheel_tag>): publish.yml's `uv publish` uploads the pure
#        py3-none-any wheel first (it sorts first), so the index can offer it
#        before the platform wheel arrives.
#
# Environment: SKILLTEST_REGISTRY_ATTEMPTS (default 10) and
# SKILLTEST_REGISTRY_INTERVAL in seconds (default 30) bound the retries.
#
# One line on success. Exit codes: 0 installed; 1 the package never arrived
# within the bound, naming it; 2 invalid invocation.
set -euo pipefail

err() {
  echo "await-registry-install: $1" >&2
  exit 1
}
usage="usage: await-registry-install.sh <npm|pypi> <target> <version> <dir>; pass the registry, a declared target triple, the published version X.Y.Z and the consumer project (npm) or venv (pypi) directory"
usage_err() {
  echo "await-registry-install: $1" >&2
  exit 2
}
[ "$#" -eq 4 ] && [ -d "$4" ] || usage_err "$usage"
registry="$1" target="$2" version="$3" dir="${4%/}"
case "$registry" in npm | pypi) ;; *) usage_err "registry '$registry' is neither npm nor pypi; $usage" ;; esac
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$ ]] || usage_err "version '$version' is not X.Y.Z; $usage"
attempts="${SKILLTEST_REGISTRY_ATTEMPTS:-10}" interval="${SKILLTEST_REGISTRY_INTERVAL:-30}"
[[ "$attempts" =~ ^[1-9][0-9]*$ && "$interval" =~ ^[0-9]+$ ]] ||
  usage_err "SKILLTEST_REGISTRY_ATTEMPTS must be a positive and SKILLTEST_REGISTRY_INTERVAL a non-negative whole number"

# The target's declared field, read without python (absent on some runners).
here="${BASH_SOURCE[0]%/*}"
[ "$here" != "${BASH_SOURCE[0]}" ] || here=.
declared() {
  awk -v target="$target" -v key="$1" '
    /^\[\[platform\]\]/ { mine = 0 }
    $1 == "target" && $3 == "\"" target "\"" { mine = 1 }
    mine && $1 == key { gsub(/"/, "", $3); print $3; exit }
  ' "$here/../release-platforms.toml"
}
case "$registry" in
  npm) field=npm_package ;;
  pypi) field=wheel_tag ;;
esac
want="$(declared "$field")" || err "could not read release-platforms.toml; restore it from a complete checkout"
[ -n "$want" ] || usage_err "release-platforms.toml declares no $field for target '$target'; pass a declared target"

# Prints what is missing after an install (nothing when complete).
missing_npm() {
  local name
  for name in @skill-test/sdk "$want"; do
    grep -Eq "\"version\": *\"$version\"" "$dir/node_modules/$name/package.json" 2>/dev/null ||
      { echo "$name@$version"; return; }
  done
}
missing_pypi() {
  local wheel
  wheel="$(find "$dir" -path "*/skilltest_sdk-$version.dist-info/WHEEL" 2>/dev/null | head -1)"
  [ -n "$wheel" ] || { echo "skilltest-sdk==$version"; return; }
  tr -d '\r' <"$wheel" | grep -Fxq "Tag: py3-none-$want" ||
    echo "the py3-none-$want platform wheel of skilltest-sdk==$version (installed: $(tr -d '\r' <"$wheel" | sed -n 's/^Tag: //p' | paste -sd ' ' -))"
}

python="$dir/bin/python"
[ ! -e "$dir/Scripts/python.exe" ] || python="$dir/Scripts/python.exe"
for ((i = 1; ; i++)); do
  case "$registry" in
    npm)
      # A fresh tree each time, so a platform package skipped on an earlier
      # attempt is resolved again rather than read back from the lockfile.
      rm -rf "$dir/node_modules" "$dir/package-lock.json" || err "could not clear $dir's node_modules; check its permissions"
      (cd "$dir" && npm install --no-audit --no-fund --prefer-online "@skill-test/sdk@$version") || true
      missing="$(missing_npm)"
      ;;
    pypi)
      uv pip install --python "$python" --refresh --reinstall "skilltest-sdk==$version" || true
      missing="$(missing_pypi)"
      ;;
  esac
  if [ -z "$missing" ]; then
    echo "await-registry-install: $registry installed $target's bundled CLI for $version (attempt $i)"
    exit 0
  fi
  [ "$i" -lt "$attempts" ] ||
    err "$missing did not install from $registry after $attempts attempts ${interval}s apart; check that publish.yml's $registry job published it, then re-run this job"
  echo "await-registry-install: waiting for $missing on $registry ($i/$attempts)…"
  sleep "$interval"
done
