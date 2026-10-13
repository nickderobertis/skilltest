#!/usr/bin/env bash
# llmlint: ignore-file[new_code_lands_in_a_project] tests scripts/install.sh, repo-level install glue that belongs to no Nx package; run workspace-wide from `just release-targets-check` (AGENTS.md: scripts/*.sh are orchestrator-independent glue).
# Test of scripts/install.sh as a user runs it, with only the host identity
# (uname, sysctl) and the GitHub download transport (curl) doubled: it installs
# a stand-in release archive on Linux and Apple Silicon macOS, takes the arm64
# binary under Rosetta, and refuses Intel macOS before downloading anything.
#
# Quiet on success, one line. On failure it prints what the installer said.
set -euo pipefail
cd "$(dirname "$0")/.." || {
  echo "install-test: cannot enter the repository root from $0; run it from a complete checkout" >&2
  exit 1
}

work="$(mktemp -d)" || {
  echo "install-test: could not create a scratch directory; check that \$TMPDIR (or /tmp) is writable and has space" >&2
  exit 1
}
trap 'rm -rf "$work" || echo "install-test: could not remove $work; delete it by hand" >&2' EXIT

fail() {
  echo "install-test: $1" >&2
  if [ -s "$work/out" ]; then
    echo "  what the installer said:" >&2
    cat "$work/out" >&2
  fi
  exit 1
}

# $1 = the asset, checksummed beside it the way release.yml uploads it.
checksum() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1"; else shasum -a 256 "$1"; fi >"$1.sha256"
}

# The release the doubled transport serves: one stand-in archive per target,
# with its checksum, under releases/download/v1.2.3/.
release="$work/release/v1.2.3"
mkdir -p "$release" "$work/src" || fail "could not create $release; check that $work is writable"
for target in x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu aarch64-apple-darwin; do
  { printf '#!/bin/sh\necho "skilltest 1.2.3 %s"\n' "$target" >"$work/src/skilltest" && chmod +x "$work/src/skilltest" &&
    tar -czf "$release/skilltest-$target.tar.gz" -C "$work/src" skilltest &&
    (cd "$release" && checksum "skilltest-$target.tar.gz"); } ||
    fail "could not pack the stand-in archive for $target; check that tar and sha256sum or shasum are on PATH"
done

# $1 = uname -s, $2 = uname -m, $3 = sysctl.proc_translated ("" = no sysctl).
# Writes a PATH directory whose uname/sysctl report that host and whose curl
# serves $release, logging every URL it is asked for.
host() {
  local bin="$work/host-$1-$2-${3:-none}"
  mkdir -p "$bin" || fail "could not create $bin; check that $work is writable"
  cat >"$bin/uname" <<EOF
#!/bin/sh
case "\$1" in -s) echo $1 ;; -m) echo $2 ;; esac
EOF
  if [ -n "$3" ]; then
    cat >"$bin/sysctl" <<EOF
#!/bin/sh
[ "\$2" = sysctl.proc_translated ] && echo $3
EOF
  else
    printf '#!/bin/sh\nexit 1\n' >"$bin/sysctl"
  fi
  cat >"$bin/curl" <<EOF
#!/bin/sh
dest=""
while [ "\$#" -gt 1 ]; do
  case "\$1" in -o) dest="\$2"; shift ;; esac
  shift
done
echo "\$1" >>"$work/fetched"
file="$work/release/\${1#https://github.com/nickderobertis/skilltest/releases/download/}"
[ -f "\$file" ] || exit 22
if [ -n "\$dest" ]; then cp "\$file" "\$dest"; else cat "\$file"; fi
EOF
  chmod +x "$bin/uname" "$bin/sysctl" "$bin/curl" || fail "could not make the host doubles in $bin executable"
  printf '%s\n' "$bin"
}

# $1 = host PATH directory. Runs the installer into a fresh directory.
install_on() {
  { : >"$work/out" && : >"$work/fetched" && rm -rf "$work/dest"; } || fail "could not reset $work; check that it is writable"
  PATH="$1:$PATH" SKILLTEST_VERSION=v1.2.3 SKILLTEST_INSTALL_DIR="$work/dest" sh scripts/install.sh >"$work/out" 2>&1
}

# $1 = what the host is, $2 = the target it must install, $3.. = host().
expect_install() {
  local what="$1" target="$2"
  shift 2
  install_on "$(host "$@")" || fail "the installer failed on $what; fix detect_target in scripts/install.sh"
  [ "$("$work/dest/skilltest")" = "skilltest 1.2.3 $target" ] ||
    fail "the installer put something other than the $target release binary in place on $what; fix the target it resolves in scripts/install.sh"
}

expect_install "x86_64 Linux" x86_64-unknown-linux-gnu Linux x86_64 ""
expect_install "aarch64 Linux" aarch64-unknown-linux-gnu Linux aarch64 ""
expect_install "Apple Silicon macOS" aarch64-apple-darwin Darwin arm64 0
expect_install "a Rosetta shell on Apple Silicon macOS" aarch64-apple-darwin Darwin x86_64 1

if install_on "$(host Darwin x86_64 0)"; then
  fail "the installer succeeded on Intel macOS, which no release ships a binary for; restore its Intel macOS refusal in scripts/install.sh"
fi
grep -Fq "Intel macOS (x86_64) is no longer supported" "$work/out" ||
  fail "the installer refused Intel macOS without saying it is no longer supported; restore that message in scripts/install.sh"
[ ! -s "$work/fetched" ] || fail "the installer fetched $(head -1 "$work/fetched") on Intel macOS before refusing; refuse in detect_target, before any download"
[ ! -e "$work/dest/skilltest" ] || fail "the installer left a binary behind on Intel macOS; refuse before installing anything"

echo "install-test: install.sh installs on Linux and Apple Silicon macOS and refuses Intel macOS before downloading"
