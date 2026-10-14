#!/usr/bin/env bash
# Installs a just-published SDK from its registry the way a consumer does, and
# retries until what installed is the target's bundled CLI, not merely the SDK.
# publish.yml's verify-windows job runs it before running the bundled CLI.
#
# Usage: await-registry-install.sh npm <target> <version> <consumer-dir>
#        await-registry-install.sh pypi <target> <version> <venv-dir>
#   npm  installs @skill-test/sdk@<version> into <consumer-dir> (an npm
#        project: it holds a package.json) until it and the target's npm_package from
#        release-platforms.toml are both in its node_modules: npm skips an
#        optional dependency the registry still 404s on without failing.
#   pypi installs skilltest-sdk==<version> into the venv at <venv-dir> (it
#        holds a pyvenv.cfg) until
#        the installed wheel is the target's platform wheel (Tag:
#        py3-none-<wheel_tag>): publish.yml's `uv publish` uploads the pure
#        py3-none-any wheel first (it sorts first), so the index can offer it
#        before the platform wheel arrives.
#
# Environment: SKILLTEST_REGISTRY_ATTEMPTS (default 10) and
# SKILLTEST_REGISTRY_INTERVAL in seconds (default 30) bound the retries.
#
# One line on success; on failure, the reason and the last attempt's npm or uv
# output. Exit codes: 0 installed; 1 the package never arrived
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
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$ ]] || usage_err "version '$version' is not X.Y.Z; $usage"
case "$registry" in
  npm) marker=package.json ;;
  pypi) marker=pyvenv.cfg ;;
  *) usage_err "registry '$registry' is neither npm nor pypi; $usage" ;;
esac
# The npm attempt clears node_modules under <dir>, so <dir> must be the
# consumer it names, never the filesystem root or an unrelated directory.
[ -n "$dir" ] && [ -f "$dir/$marker" ] ||
  usage_err "'$4' has no $marker, so it is not the $registry consumer to install into; create it first (npm init -y, or uv venv)"
attempts="${SKILLTEST_REGISTRY_ATTEMPTS:-10}" interval="${SKILLTEST_REGISTRY_INTERVAL:-30}"
[[ "$attempts" =~ ^[1-9][0-9]*$ && "$interval" =~ ^[0-9]+$ ]] ||
  usage_err "SKILLTEST_REGISTRY_ATTEMPTS must be a positive and SKILLTEST_REGISTRY_INTERVAL a non-negative whole number; unset them to use the defaults (10 attempts, 30 s apart)"

[[ "$target" =~ ^[a-z0-9_]+(-[a-z0-9_]+)+$ ]] || usage_err "target '$target' is not a target triple; $usage"

# The target's declared field, read without python (absent on some runners).
# Only the exact `key = "value"` lines release-platforms.toml uses are read, and
# a target declared twice, or a field declared twice or written any other way
# in its entry, is refused rather than resolved to one of its readings.
here="${BASH_SOURCE[0]%/*}"
[ "$here" != "${BASH_SOURCE[0]}" ] || here=.
declared() {
  awk -v target="$target" -v key="$1" '
    /^\[\[platform\]\]$/ { mine = 0; next }
    $1 == "target" { mine = ($0 == "target = \"" target "\""); entries += mine; next }
    mine && $1 == key {
      if ($0 !~ ("^" key " = \"[^\"]*\"$")) { malformed = 1; next }
      values++
      value = $0
      sub(/^[^"]*"/, "", value)
      sub(/"$/, "", value)
    }
    END {
      if (entries > 1) exit 3
      if (malformed) exit 4
      if (values > 1) exit 5
      print value
    }
  ' "$here/../release-platforms.toml"
}
# Each value names a node_modules path or a wheel tag, so it is held to the
# grammar scripts/stage-and-verify-release-product.py holds the file to
# (PLATFORM_GRAMMAR; await-registry-install-test.sh fails if they differ).
case "$registry" in
  npm) field=npm_package grammar='^@skill-test/cli-[a-z0-9]+-[a-z0-9]+$' ;;
  pypi) field=wheel_tag grammar='^[a-z0-9_]+$' ;;
esac
status=0
want="$(declared "$field")" || status=$?
case "$status" in
  0) ;;
  3) err "release-platforms.toml declares target $target more than once; keep one [[platform]] entry per target" ;;
  4) err "release-platforms.toml writes $target's $field other than as $field = \"<value>\"; fix that [[platform]] entry" ;;
  5) err "release-platforms.toml declares $field more than once for $target; keep one in that [[platform]] entry" ;;
  *) err "could not read release-platforms.toml; restore it from a complete checkout" ;;
esac
[ -n "$want" ] || usage_err "release-platforms.toml declares no $field for target '$target'; pass a declared target"
[[ "$want" =~ $grammar ]] || err "release-platforms.toml declares $field '$want' for $target, which is not a valid $field; fix that [[platform]] entry"

# A package npm skipped has no manifest, which is a registry delay worth
# retrying; a manifest that cannot be read, is not JSON, or names another
# package or version is not, so it stops the script naming what was wrong.
missing_npm() {
  local name status
  for name in @skill-test/sdk "$want"; do
    status=0
    node -e '
      const [path, name, version] = process.argv.slice(1);
      const refuse = (why) => {
        console.error(path + " " + why);
        process.exit(3);
      };
      let text;
      try {
        text = require("fs").readFileSync(path, "utf8");
      } catch (error) {
        if (error.code === "ENOENT") process.exit(1);
        refuse("could not be read: " + error.message);
      }
      let manifest;
      try {
        manifest = JSON.parse(text);
      } catch (error) {
        refuse("is not JSON: " + error.message);
      }
      if (manifest === null || typeof manifest !== "object" || Array.isArray(manifest)) {
        refuse("is not a JSON object");
      }
      if (manifest.name !== name || manifest.version !== version) {
        refuse("is " + manifest.name + "@" + manifest.version + ", not " + name + "@" + version);
      }
    ' "$dir/node_modules/$name/package.json" "$name" "$version" || status=$?
    case "$status" in
      0) ;;
      1) echo "$name@$version" && return ;;
      *) err "npm installed an unusable $name (reason above), which is not a registry delay; check $dir's permissions and the SDK's pinned optionalDependencies, then re-run this job" ;;
    esac
  done
}
missing_pypi() {
  local found wheel tags
  found="$(find "$dir" -path "*/skilltest_sdk-$version.dist-info/WHEEL")" ||
    err "could not search $dir for the installed skilltest-sdk (reason above); check the venv's permissions"
  wheel="${found%%$'\n'*}"
  [ -n "$wheel" ] || { echo "skilltest-sdk==$version"; return; }
  tags="$(tr -d '\r' <"$wheel" | sed -n 's/^Tag: //p')" ||
    err "could not read $wheel (reason above); check the venv's permissions"
  grep -Fxq "py3-none-$want" <<<"$tags" ||
    echo "the py3-none-$want platform wheel of skilltest-sdk==$version (installed: ${tags//$'\n'/ })"
}

python="$dir/bin/python"
[ ! -e "$dir/Scripts/python.exe" ] || python="$dir/Scripts/python.exe"
for ((i = 1; ; i++)); do
  case "$registry" in
    npm)
      # A fresh tree each time, so a platform package skipped on an earlier
      # attempt is resolved again rather than read back from the lockfile.
      rm -rf "$dir/node_modules" "$dir/package-lock.json" || err "could not clear $dir's node_modules; check its permissions"
      if said="$(cd "$dir" && npm install --no-audit --no-fund --prefer-online "@skill-test/sdk@$version" 2>&1)"; then
        missing="$(missing_npm)"
      else
        missing="@skill-test/sdk@$version"
      fi
      ;;
    pypi)
      # A failed reinstall can leave an earlier attempt's wheel behind, so only
      # a successful install is inspected.
      if said="$(uv pip install --python "$python" --refresh --reinstall "skilltest-sdk==$version" 2>&1)"; then
        missing="$(missing_pypi)"
      else
        missing="skilltest-sdk==$version"
      fi
      ;;
  esac
  if [ -z "$missing" ]; then
    echo "await-registry-install: $registry installed $target's bundled CLI for $version (attempt $i)"
    exit 0
  fi
  if [ "$i" -ge "$attempts" ]; then
    printf '%s\n' "the last $registry install said:" "$said" >&2
    err "$missing did not install from $registry after $attempts attempts ${interval}s apart; check that publish.yml's $registry job published it, then re-run this job"
  fi
  sleep "$interval" || err "could not wait ${interval}s between attempts; check that sleep is on PATH, then re-run this job"
done
