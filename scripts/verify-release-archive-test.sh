#!/usr/bin/env bash
# llmlint: ignore-file[new_code_lands_in_a_project] tests scripts/verify-release-archive.sh, repo-level release glue that belongs to no Nx package; run workspace-wide from `just check` (AGENTS.md: scripts/*.sh are orchestrator-independent glue).
# Test of scripts/verify-release-archive.sh on archives packed here the way
# release.yml's upload action packs them (binary at the root, a .sha256 beside
# it), from a stand-in binary: green on a good .tar.gz and a good Windows .zip,
# red with the reason on each way a release archive can be wrong.
#
# Quiet on success, one line. On failure it prints what the verifier said.
set -euo pipefail
cd "$(dirname "$0")/.." || {
  echo "verify-release-archive-test: cannot enter the repository root from $0; run it from a complete checkout" >&2
  exit 1
}

work="$(mktemp -d)" || {
  echo "verify-release-archive-test: could not create a scratch directory; check that \$TMPDIR (or /tmp) is writable and has space" >&2
  exit 1
}
trap 'rm -rf "$work" || echo "verify-release-archive-test: could not remove $work; delete it by hand" >&2' EXIT

fail() {
  echo "verify-release-archive-test: $1" >&2
  if [ -s "$work/out" ]; then
    echo "  what the verifier said:" >&2
    cat "$work/out" >&2 || echo "  (unreadable: $work/out; rerun scripts/verify-release-archive.sh by hand with the arguments above to see it)" >&2
  fi
  exit 1
}

# $1 = the asset, checksummed in the current directory the way the upload
# action writes it. sha256sum where present, else macOS's shasum.
checksum() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1"; else shasum -a 256 "$1"; fi >"$1.sha256"
}

# $1 = target, $2 = bin, $3 = the version the stand-in reports. Packs
# $work/<target>/ the way release.yml publishes it.
pack() {
  local target="$1" bin="$2" dir="$work/$1" asset
  { rm -rf "$dir" "$work/src" && mkdir -p "$dir" "$work/src"; } || fail "could not create $dir; check that $work is writable"
  { printf '#!/bin/sh\necho "skilltest %s"\n' "$3" >"$work/src/$bin" && chmod +x "$work/src/$bin"; } ||
    fail "could not write the stand-in $bin into $work/src; check that $work is writable"
  case "$bin" in
    *.exe)
      asset="skilltest-$target.zip"
      python3 - "$work/src/$bin" "$dir/$asset" "$bin" <<'EOF' || fail "could not zip the stand-in; check that python3 is on PATH"
import sys, zipfile
src, dest, name = sys.argv[1:]
info = zipfile.ZipInfo(name)
info.external_attr = 0o100755 << 16
with open(src, "rb") as f, zipfile.ZipFile(dest, "w") as z:
    z.writestr(info, f.read())
EOF
      ;;
    *)
      asset="skilltest-$target.tar.gz"
      tar -czf "$dir/$asset" -C "$work/src" "$bin" || fail "could not tar the stand-in; check that tar and gzip are on PATH and $work is writable"
      ;;
  esac
  (cd "$dir" && checksum "$asset") || fail "could not checksum $asset; check that sha256sum or shasum is on PATH"
}

# Runs the verifier, on $verify_path instead of PATH when a case sets it.
verify_path=""
verify() { PATH="${verify_path:-$PATH}" "$BASH" scripts/verify-release-archive.sh "$@" >"$work/out" 2>&1; }

# Prints a directory of links to the verifier's tools minus those named, for a
# case that needs one missing.
tools_without() {
  local dir tool found
  dir="$(mktemp -d "$work/path.XXXXXX")" || fail "could not create a tool directory under $work; check that it is writable"
  for tool in tr rm mkdir cat sed unzip tar gzip sha256sum shasum; do
    case " $* " in *" $tool "*) continue ;; esac
    found="$(type -P "$tool")" || continue
    ln -s "$found" "$dir/$tool" || fail "could not link $tool into $dir; check that $work is writable"
  done
  printf '%s\n' "$dir"
}

# $1 = what is wrong, $2 = what the verifier must say, rest = its arguments.
expect_red() {
  local what="$1" says="$2"
  shift 2
  if verify "$@"; then fail "the verifier passed an archive where $what; restore the check in scripts/verify-release-archive.sh that refuses it"; fi
  grep -Fq -- "$says" "$work/out" ||
    fail "the verifier refused an archive where $what, but not for that reason (expected it to say '$says'); fix the branch of scripts/verify-release-archive.sh that now reports it, or this case if it no longer produces that fault"
}

pack x86_64-unknown-linux-gnu skilltest 1.2.3
verify "$work/x86_64-unknown-linux-gnu" x86_64-unknown-linux-gnu skilltest 1.2.3 ||
  fail "the verifier refused a good .tar.gz; fix the check in scripts/verify-release-archive.sh named below, or this test's pack() if release.yml's archive shape changed"

for target in x86_64-pc-windows-msvc aarch64-pc-windows-msvc; do
  pack "$target" skilltest.exe 1.2.3
  verify "$work/$target" "$target" skilltest.exe 1.2.3 || fail "the verifier refused a good $target .zip; fix the check in scripts/verify-release-archive.sh named below, or this test's pack() if release.yml's archive shape changed"
done

expect_red "the binary is from another release" "reports 'skilltest 1.2.3', not 'skilltest 1.2.2'" \
  "$work/x86_64-pc-windows-msvc" x86_64-pc-windows-msvc skilltest.exe 1.2.2

pack x86_64-pc-windows-msvc skilltest.exe 1.2.3
printf 'tampered' >>"$work/x86_64-pc-windows-msvc/skilltest-x86_64-pc-windows-msvc.zip" ||
  fail "could not tamper with the stand-in zip; check that $work is writable"
expect_red "the archive does not match its checksum" "checksum mismatch for skilltest-x86_64-pc-windows-msvc.zip" \
  "$work/x86_64-pc-windows-msvc" x86_64-pc-windows-msvc skilltest.exe 1.2.3

pack aarch64-pc-windows-msvc skilltest 1.2.3
{
  mv "$work/aarch64-pc-windows-msvc/skilltest-aarch64-pc-windows-msvc.tar.gz" "$work/aarch64-pc-windows-msvc/skilltest-aarch64-pc-windows-msvc.zip" &&
    mv "$work/aarch64-pc-windows-msvc/skilltest-aarch64-pc-windows-msvc.tar.gz.sha256" "$work/aarch64-pc-windows-msvc/skilltest-aarch64-pc-windows-msvc.zip.sha256"
} || fail "could not rename the stand-in tarball as a zip; check that $work is writable"
expect_red "the Windows archive is not a zip" "could not list skilltest-aarch64-pc-windows-msvc.zip" \
  "$work/aarch64-pc-windows-msvc" aarch64-pc-windows-msvc skilltest.exe 1.2.3

pack x86_64-pc-windows-msvc skilltest.exe 1.2.3
python3 - "$work/x86_64-pc-windows-msvc/skilltest-x86_64-pc-windows-msvc.zip" <<'EOF' || fail "could not rewrite the stand-in zip; check that python3 is on PATH and $work is writable"
import sys, zipfile
with zipfile.ZipFile(sys.argv[1], "w") as z:
    z.writestr("skilltest", "#!/bin/sh\n")
EOF
(cd "$work/x86_64-pc-windows-msvc" && checksum skilltest-x86_64-pc-windows-msvc.zip) ||
  fail "could not re-checksum the stand-in zip; check that sha256sum or shasum is on PATH"
expect_red "the Windows archive holds its binary without .exe" "holds skilltest rather than only skilltest.exe at its root" \
  "$work/x86_64-pc-windows-msvc" x86_64-pc-windows-msvc skilltest.exe 1.2.3

rm "$work/x86_64-unknown-linux-gnu/skilltest-x86_64-unknown-linux-gnu.tar.gz.sha256" ||
  fail "could not remove the stand-in checksum; check that $work is writable"
expect_red "the checksum was not uploaded" "has no skilltest-x86_64-unknown-linux-gnu.tar.gz.sha256" \
  "$work/x86_64-unknown-linux-gnu" x86_64-unknown-linux-gnu skilltest 1.2.3

pack x86_64-unknown-linux-gnu skilltest 1.2.3
{ printf '#!/bin/sh\nexit 3\n' >"$work/src/skilltest" && tar -czf "$work/x86_64-unknown-linux-gnu/skilltest-x86_64-unknown-linux-gnu.tar.gz" -C "$work/src" skilltest &&
  (cd "$work/x86_64-unknown-linux-gnu" && checksum skilltest-x86_64-unknown-linux-gnu.tar.gz); } ||
  fail "could not repack the stand-in as a failing binary; check that tar is on PATH and $work is writable"
expect_red "the binary does not run" "skilltest from skilltest-x86_64-unknown-linux-gnu.tar.gz did not run" \
  "$work/x86_64-unknown-linux-gnu" x86_64-unknown-linux-gnu skilltest 1.2.3

expect_red "the binary name escapes the archive" "bin '../skilltest' is neither skilltest nor skilltest.exe" \
  "$work/x86_64-unknown-linux-gnu" x86_64-unknown-linux-gnu ../skilltest 1.2.3

pack x86_64-pc-windows-msvc skilltest.exe 1.2.3
python3 - "$work/x86_64-pc-windows-msvc/skilltest-x86_64-pc-windows-msvc.zip" <<'PY' || fail "could not rewrite the stand-in zip; check that python3 is on PATH and $work is writable"
import sys, zipfile
with zipfile.ZipFile(sys.argv[1], "w") as z:
    z.writestr("skilltest.exe", "#!/bin/sh\n")
    z.writestr("../escaped", "x")
PY
(cd "$work/x86_64-pc-windows-msvc" && checksum skilltest-x86_64-pc-windows-msvc.zip) ||
  fail "could not re-checksum the stand-in zip; check that sha256sum or shasum is on PATH"
expect_red "the archive carries an entry that escapes its root" "holds skilltest.exe ../escaped rather than only skilltest.exe" \
  "$work/x86_64-pc-windows-msvc" x86_64-pc-windows-msvc skilltest.exe 1.2.3
[ ! -e "$work/x86_64-pc-windows-msvc/escaped" ] || fail "the verifier extracted an escaping entry before refusing it; list the archive before extracting it"

pack x86_64-unknown-linux-gnu skilltest 1.2.3
{ printf 'not a gzip stream' >"$work/x86_64-unknown-linux-gnu/skilltest-x86_64-unknown-linux-gnu.tar.gz" &&
  (cd "$work/x86_64-unknown-linux-gnu" && checksum skilltest-x86_64-unknown-linux-gnu.tar.gz); } ||
  fail "could not corrupt the stand-in tarball; check that $work is writable"
expect_red "the tarball is corrupt" "could not list skilltest-x86_64-unknown-linux-gnu.tar.gz" \
  "$work/x86_64-unknown-linux-gnu" x86_64-unknown-linux-gnu skilltest 1.2.3

pack x86_64-unknown-linux-gnu skilltest 1.2.3
{ rm -f "$work/src/skilltest" && ln -s /bin/true "$work/src/skilltest" &&
  tar -czf "$work/x86_64-unknown-linux-gnu/skilltest-x86_64-unknown-linux-gnu.tar.gz" -C "$work/src" skilltest &&
  (cd "$work/x86_64-unknown-linux-gnu" && checksum skilltest-x86_64-unknown-linux-gnu.tar.gz); } ||
  fail "could not repack the stand-in as a symlink; check that tar is on PATH and $work is writable"
expect_red "the binary is a link to a file outside the archive" "skilltest-x86_64-unknown-linux-gnu.tar.gz's skilltest is not a regular file" \
  "$work/x86_64-unknown-linux-gnu" x86_64-unknown-linux-gnu skilltest 1.2.3

pack x86_64-unknown-linux-gnu skilltest 1.2.3
expect_red "the archive was never uploaded" "has no skilltest-aarch64-unknown-linux-gnu.tar.gz" \
  "$work/x86_64-unknown-linux-gnu" aarch64-unknown-linux-gnu skilltest 1.2.3

expect_red "the target is not a triple" "target 'x86_64 linux' is not a target triple" \
  "$work/x86_64-unknown-linux-gnu" "x86_64 linux" skilltest 1.2.3

expect_red "the version is not X.Y.Z" "version 'v1.2.3' is not X.Y.Z" \
  "$work/x86_64-unknown-linux-gnu" x86_64-unknown-linux-gnu skilltest v1.2.3

pack x86_64-unknown-linux-gnu skilltest 1.2.3
verify_path="$(tools_without sha256sum)"
verify "$work/x86_64-unknown-linux-gnu" x86_64-unknown-linux-gnu skilltest 1.2.3 ||
  fail "the verifier refused a good .tar.gz where only shasum can hash it; fix the shasum fallback in scripts/verify-release-archive.sh"
verify_path="$(tools_without sha256sum shasum)"
expect_red "no checksum tool is on PATH" "shasum could not hash $work/x86_64-unknown-linux-gnu/skilltest-x86_64-unknown-linux-gnu.tar.gz, and sha256sum is not on PATH" \
  "$work/x86_64-unknown-linux-gnu" x86_64-unknown-linux-gnu skilltest 1.2.3

pack x86_64-pc-windows-msvc skilltest.exe 1.2.3
verify_path="$(tools_without unzip)"
expect_red "unzip is not on PATH" "unzip is not on PATH, so skilltest-x86_64-pc-windows-msvc.zip cannot be opened" \
  "$work/x86_64-pc-windows-msvc" x86_64-pc-windows-msvc skilltest.exe 1.2.3
verify_path=""

python3 - "$work/x86_64-pc-windows-msvc/skilltest-x86_64-pc-windows-msvc.zip" <<'PY' || fail "could not corrupt the stand-in zip; check that python3 is on PATH and $work is writable"
import sys, zipfile
with zipfile.ZipFile(sys.argv[1], "w") as z:
    z.writestr("skilltest.exe", "#!/bin/sh\necho skilltest 1.2.3\n")
data = bytearray(open(sys.argv[1], "rb").read())
at = data.index(b"echo skilltest")
data[at] ^= 0xFF
open(sys.argv[1], "wb").write(bytes(data))
PY
(cd "$work/x86_64-pc-windows-msvc" && checksum skilltest-x86_64-pc-windows-msvc.zip) ||
  fail "could not re-checksum the stand-in zip; check that sha256sum or shasum is on PATH"
expect_red "the zip lists cleanly but its member is corrupt" "could not extract skilltest-x86_64-pc-windows-msvc.zip" \
  "$work/x86_64-pc-windows-msvc" x86_64-pc-windows-msvc skilltest.exe 1.2.3

pack x86_64-unknown-linux-gnu skilltest 1.2.3
python3 - "$work/x86_64-unknown-linux-gnu/skilltest-x86_64-unknown-linux-gnu.tar.gz" <<'PY' || fail "could not write the stand-in hard-link tarball; check that python3 is on PATH and $work is writable"
import sys, tarfile
with tarfile.open(sys.argv[1], "w:gz") as t:
    link = tarfile.TarInfo("skilltest")
    link.type = tarfile.LNKTYPE
    link.linkname = "absent"
    t.addfile(link)
PY
(cd "$work/x86_64-unknown-linux-gnu" && checksum skilltest-x86_64-unknown-linux-gnu.tar.gz) ||
  fail "could not re-checksum the stand-in tarball; check that sha256sum or shasum is on PATH"
expect_red "the tarball lists cleanly but cannot be extracted" "could not extract skilltest-x86_64-unknown-linux-gnu.tar.gz" \
  "$work/x86_64-unknown-linux-gnu" x86_64-unknown-linux-gnu skilltest 1.2.3

# Unreadable and unwritable files stop nothing for root, so these cases need
# an ordinary user, as every CI runner and a developer's shell are.
if [ "$(id -u)" != 0 ]; then
  pack x86_64-unknown-linux-gnu skilltest 1.2.3
  chmod 000 "$work/x86_64-unknown-linux-gnu/skilltest-x86_64-unknown-linux-gnu.tar.gz" || fail "could not make the stand-in tarball unreadable"
  expect_red "the archive cannot be read" "could not hash $work/x86_64-unknown-linux-gnu/skilltest-x86_64-unknown-linux-gnu.tar.gz" \
    "$work/x86_64-unknown-linux-gnu" x86_64-unknown-linux-gnu skilltest 1.2.3

  pack x86_64-unknown-linux-gnu skilltest 1.2.3
  chmod 000 "$work/x86_64-unknown-linux-gnu/skilltest-x86_64-unknown-linux-gnu.tar.gz.sha256" || fail "could not make the stand-in checksum unreadable"
  expect_red "the checksum cannot be read" "could not read $work/x86_64-unknown-linux-gnu/skilltest-x86_64-unknown-linux-gnu.tar.gz.sha256" \
    "$work/x86_64-unknown-linux-gnu" x86_64-unknown-linux-gnu skilltest 1.2.3

  pack x86_64-unknown-linux-gnu skilltest 1.2.3
  chmod 555 "$work/x86_64-unknown-linux-gnu" || fail "could not make the stand-in download directory read-only"
  expect_red "the download directory is read-only" "could not prepare $work/x86_64-unknown-linux-gnu/extracted" \
    "$work/x86_64-unknown-linux-gnu" x86_64-unknown-linux-gnu skilltest 1.2.3
  chmod 755 "$work/x86_64-unknown-linux-gnu" || fail "could not make $work/x86_64-unknown-linux-gnu writable again; delete it by hand"
fi

pack x86_64-unknown-linux-gnu skilltest 1.2.3
expect_red "it was called without a version" "usage: verify-release-archive.sh" \
  "$work/x86_64-unknown-linux-gnu" x86_64-unknown-linux-gnu skilltest

echo "verify-release-archive-test: the verifier passes good .tar.gz and Windows .zip archives and refuses each way one can be wrong"
