#!/usr/bin/env bash
# Test of scripts/install.sh as a user runs it, with only the host identity
# (uname, sysctl) and the GitHub download transport (curl) doubled: it installs
# a stand-in release archive on Linux and Apple Silicon macOS, takes the arm64
# binary under Rosetta, and refuses Intel macOS before downloading anything. It
# installs from a release offering either checksum name scripts/release-checksums.txt
# declares on its own (v0.1.0-v0.13.0 carry only skilltest-<target>.sha256),
# refuses one offering neither, and refuses a wrong checksum under either name.
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
    cat "$work/out" >&2 || echo "  (unreadable: $work/out; rerun sh scripts/install.sh by hand with the doubled PATH to see it)" >&2
  fi
  exit 1
}

# $1 = the target. Prints the checksum file names a release carries beside
# its archive, read from the declaration the release scripts share.
checksum_names() {
  bash scripts/verify-release-archive.sh --checksum-names "$1" skilltest ||
    fail "could not read the declared checksum names; restore scripts/release-checksums.txt"
}

# $1 = the release tag, $2 = the target, $3.. = the checksum file names to
# offer beside its archive (none: offer no checksum). Packs a stand-in archive
# under releases/download/<tag>/ for the doubled transport to serve.
publish() {
  local release="$work/release/$1" target="$2" name
  shift 2
  mkdir -p "$release" "$work/src" || fail "could not create $release; check that $work is writable"
  { printf '#!/bin/sh\necho "skilltest 1.2.3 %s"\n' "$target" >"$work/src/skilltest" && chmod +x "$work/src/skilltest" &&
    tar -czf "$release/skilltest-$target.tar.gz" -C "$work/src" skilltest; } ||
    fail "could not pack the stand-in archive for $target; check that tar is on PATH"
  for name in "$@"; do
    (cd "$release" && if command -v sha256sum >/dev/null 2>&1; then sha256sum "skilltest-$target.tar.gz"; else shasum -a 256 "skilltest-$target.tar.gz"; fi >"$name") ||
      fail "could not checksum the stand-in archive for $target; check that sha256sum or shasum is on PATH"
  done
}

# The release every host case installs: one stand-in archive per target, with
# every declared checksum, under releases/download/v1.2.3/.
for target in x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu aarch64-apple-darwin; do
  mapfile -t names < <(checksum_names "$target")
  publish v1.2.3 "$target" "${names[@]}"
done

# $1 = uname -s, $2 = uname -m, $3 = sysctl.proc_translated ("" = no sysctl).
# Sets $bin to a PATH directory whose uname/sysctl report that host and whose
# curl serves $release, logging every URL it is asked for. Called directly, not
# in a command substitution, so a failed write stops the test.
host() {
  bin="$work/host-$1-$2-${3:-none}"
  mkdir -p "$bin" || fail "could not create $bin; check that $work is writable"
  cat >"$bin/uname" <<EOF || fail "could not write the uname double into $bin; check that $work is writable"
#!/bin/sh
case "\$1" in -s) echo '$1' ;; -m) echo '$2' ;; esac
EOF
  if [ -n "$3" ]; then
    cat >"$bin/sysctl" <<EOF || fail "could not write the sysctl double into $bin; check that $work is writable"
#!/bin/sh
[ "\$2" = sysctl.proc_translated ] && echo '$3'
EOF
  else
    printf '#!/bin/sh\nexit 1\n' >"$bin/sysctl" || fail "could not write the sysctl double into $bin; check that $work is writable"
  fi
  # The transport double fails the download (as curl -f does) whenever it
  # cannot log the URL or serve the file, so no fetch goes unrecorded.
  cat >"$bin/curl" <<EOF || fail "could not write the curl double into $bin; check that $work is writable"
#!/bin/sh
dest=""
while [ "\$#" -gt 1 ]; do
  case "\$1" in -o) dest="\$2"; shift ;; esac
  shift
done
echo "\$1" >>"$work/fetched" || exit 23
file="$work/release/\${1#https://github.com/nickderobertis/skilltest/releases/download/}"
[ -f "\$file" ] || exit 22
if [ -n "\$dest" ]; then cp "\$file" "\$dest" || exit 23; else cat "\$file" || exit 23; fi
EOF
  chmod +x "$bin/uname" "$bin/sysctl" "$bin/curl" || fail "could not make the host doubles in $bin executable; check that $work is on a filesystem that honors chmod, or run this test from a POSIX checkout"
}

# Runs the installer on the host host() last set up, into a fresh directory.
install_on() {
  { : >"$work/out" && : >"$work/fetched" && rm -rf "$work/dest"; } || fail "could not reset $work; check that it is writable"
  PATH="$bin:$PATH" SKILLTEST_VERSION="${tag:-v1.2.3}" SKILLTEST_INSTALL_DIR="$work/dest" sh scripts/install.sh >"$work/out" 2>&1
}

# $1 = what the host is, $2 = the target it must install, $3.. = host().
expect_install() {
  local what="$1" target="$2"
  shift 2
  host "$@"
  install_on || fail "the installer failed on $what; fix detect_target in scripts/install.sh"
  [ "$("$work/dest/skilltest")" = "skilltest 1.2.3 $target" ] ||
    fail "the installer put something other than the $target release binary in place on $what; fix the target it resolves in scripts/install.sh"
}

expect_install "x86_64 Linux" x86_64-unknown-linux-gnu Linux x86_64 ""
expect_install "aarch64 Linux" aarch64-unknown-linux-gnu Linux aarch64 ""
expect_install "Apple Silicon macOS" aarch64-apple-darwin Darwin arm64 0
expect_install "a Rosetta shell on Apple Silicon macOS" aarch64-apple-darwin Darwin x86_64 1

host Darwin x86_64 0
if install_on; then
  fail "the installer succeeded on Intel macOS, which no release ships a binary for; restore its Intel macOS refusal in scripts/install.sh"
fi
grep -Fq "Intel macOS (x86_64) is no longer supported" "$work/out" ||
  fail "the installer refused Intel macOS without saying it is no longer supported; restore that message in scripts/install.sh"
[ ! -s "$work/fetched" ] || fail "the installer fetched $(head -1 "$work/fetched") on Intel macOS before refusing; refuse in detect_target, before any download"
[ ! -e "$work/dest/skilltest" ] || fail "the installer left a binary behind on Intel macOS; refuse before installing anything"

# Checksum names: each declared name alone is enough, and the installer asks
# for no checksum name the declaration does not state.
target=x86_64-unknown-linux-gnu
mapfile -t names < <(checksum_names "$target")
[ "${#names[@]}" -eq 2 ] && [ "${names[0]}" = "skilltest-$target.tar.gz.sha256" ] && [ "${names[1]}" = "skilltest-$target.sha256" ] ||
  fail "scripts/release-checksums.txt declares ${names[*]}, not the agreed skilltest-$target.tar.gz.sha256 and skilltest-$target.sha256 that install.sh fetches; restore the declaration, or change install.sh with it"
host Linux x86_64 ""
for i in "${!names[@]}"; do
  tag="v0.0.$((i + 1))"
  publish "$tag" "$target" "${names[$i]}"
  install_on || fail "the installer failed on a release offering only ${names[$i]}; fetch that name in scripts/install.sh"
  [ "$("$work/dest/skilltest")" = "skilltest 1.2.3 $target" ] ||
    fail "the installer put something other than the release binary in place from a release offering only ${names[$i]}"
  while read -r url; do
    case "${url##*/}" in "skilltest-$target.tar.gz" | "${names[0]}" | "${names[1]}") ;;
    *) fail "the installer fetched $url, which is not the archive or a declared checksum name; fix scripts/install.sh's checksum names" ;;
    esac
  done <"$work/fetched"

  tag="v0.1.$((i + 1))"
  publish "$tag" "$target"
  printf '%064d  skilltest-%s.tar.gz\n' 0 "$target" >"$work/release/$tag/${names[$i]}" ||
    fail "could not write a wrong ${names[$i]}; check that $work is writable"
  if install_on; then fail "the installer accepted an archive its ${names[$i]} does not match; restore the checksum comparison in scripts/install.sh"; fi
  grep -Fq "checksum mismatch for skilltest-$target.tar.gz (its ${names[$i]} says" "$work/out" ||
    fail "the installer refused a wrong ${names[$i]} without naming it; restore the mismatch message in scripts/install.sh"
  [ ! -e "$work/dest/skilltest" ] || fail "the installer placed a binary whose ${names[$i]} does not match"
done

tag=v0.2.0
publish "$tag" "$target"
if install_on; then fail "the installer installed from a release offering no checksum; it must refuse an archive it cannot verify"; fi
grep -Fq "neither checksum ${names[0]} nor ${names[1]}" "$work/out" ||
  fail "the installer refused a release with no checksum without naming the checksum it looked for; restore that message in scripts/install.sh"
[ ! -e "$work/dest/skilltest" ] || fail "the installer placed an unverified binary from a release offering no checksum"
unset tag

echo "install-test: install.sh installs on Linux and Apple Silicon macOS from either checksum name, refuses a missing or wrong checksum, and refuses Intel macOS before downloading"
