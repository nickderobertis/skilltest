#!/usr/bin/env bash
# Test of scripts/check-release-checksum-docs.sh: green on this checkout and on
# a copy of it, red naming the file and the checksum name when a doc drops a
# declared name, when it states one the declaration does not carry, and when
# the declaration changes without the docs. `just release-targets-check`
# (every `just check` tier) runs it.
#
# Quiet on success, one line. On failure it prints what the gate said.
set -euo pipefail
cd "$(dirname "$0")/.." || {
  echo "check-release-checksum-docs-test: cannot enter the repository root from $0; run it from a complete checkout" >&2
  exit 1
}
gate="$PWD/scripts/check-release-checksum-docs.sh"
bt='`'

work="$(mktemp -d)" || {
  echo "check-release-checksum-docs-test: could not create a scratch directory; check that \$TMPDIR (or /tmp) is writable and has space" >&2
  exit 1
}
trap 'rm -rf "$work" || echo "check-release-checksum-docs-test: could not remove $work; delete it by hand" >&2' EXIT

fail() {
  echo "check-release-checksum-docs-test: $1" >&2
  if [ -s "$work/out" ]; then
    echo "  what the gate said:" >&2
    cat "$work/out" >&2 || echo "  (unreadable: $work/out)" >&2
  fi
  echo "  fix scripts/check-release-checksum-docs.sh (or this test, if the contract changed on purpose), then rerun just release-targets-check" >&2
  exit 1
}

copy() {
  if ! { rm -rf "$work/tree" && mkdir -p "$work/tree/scripts" &&
    cp README.md AGENTS.md release-targets.toml "$work/tree/" &&
    cp scripts/release-checksums.txt "$work/tree/scripts/"; }; then
    fail "could not copy the docs into $work/tree; check that $work is writable"
  fi
}

# expect_red <what> <message>: the gate run on the copy fails saying <message>.
expect_red() {
  : >"$work/out" || fail "could not reset $work/out; check that $work is writable"
  if bash "$gate" "$work/tree" >"$work/out" 2>&1; then
    fail "the gate passed when $1"
  fi
  grep -Fq -- "$2" "$work/out" || fail "the gate failed when $1, but without saying: $2"
}

bash "$gate" >"$work/out" 2>&1 || fail "the gate fails on this checkout"
copy
bash "$gate" "$work/tree" >"$work/out" 2>&1 || fail "the gate fails on an unchanged copy of this checkout"

# A doc drops the name every release before #80 carried.
copy
sed -i.bak "s/${bt}skilltest-<target>\\.sha256${bt}/${bt}skilltest-<target>${bt}/" "$work/tree/README.md" ||
  fail "could not edit the copied README.md"
expect_red "README.md no longer names skilltest-<target>.sha256" \
  "README.md does not name ${bt}skilltest-<target>.sha256${bt}"

# A doc names a checksum file no release carries.
copy
printf '\nOld name: %sskilltest-<target>.zip.sha256sum.sha256%s.\n' "$bt" "$bt" >>"$work/tree/AGENTS.md" ||
  fail "could not edit the copied AGENTS.md"
expect_red "AGENTS.md names an undeclared checksum" \
  "AGENTS.md names ${bt}skilltest-<target>.zip.sha256sum.sha256${bt}, which scripts/release-checksums.txt does not declare"

# The declaration changes a name and the docs do not follow.
copy
sed -i.bak 's/^skilltest-{target}\.sha256$/skilltest-{target}.checksum.sha256/' "$work/tree/scripts/release-checksums.txt" ||
  fail "could not edit the copied declaration"
grep -Fxq 'skilltest-{target}.checksum.sha256' "$work/tree/scripts/release-checksums.txt" ||
  fail "scripts/release-checksums.txt no longer declares skilltest-{target}.sha256; update this test with the contract"
expect_red "the declaration renamed a checksum the docs still state" \
  "does not name ${bt}skilltest-<target>.checksum.sha256${bt}"

copy
rm "$work/tree/scripts/release-checksums.txt" || fail "could not remove the copied declaration"
expect_red "scripts/release-checksums.txt is missing" "restore scripts/release-checksums.txt"

echo "check-release-checksum-docs-test: ok"
