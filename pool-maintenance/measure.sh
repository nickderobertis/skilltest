#!/usr/bin/env bash
# Measures `skilltest-pool-maintenance-seconds` (budgets.yaml beside this script):
# the wall clock of one steady-state pass of the declared pool maintenance
# (`just pool-maintenance`), facing one ordinary run's worth of superseded builds.
#
#   (a) one declared pass, untimed, clears any older backlog;
#   (b) the workspace's test targets are built under an alternative profile (each
#       of this repository's own crates with line-table debuginfo), giving those
#       crates new metadata hashes, and exactly the entries that build created are
#       backdated past the 2-day window and older than every other entry, so they
#       stand in for a run's builds that have since been superseded;
#   (c) the ordinary gate build of the same targets, so each unit's newest build
#       is the ordinary one;
#   (d) one declared pass, timed: the pruner reports its own seconds to
#       ONEBUDGETSPEC_RESULT. Only this pass is the figure; the backlog pass's
#       seconds are appended to the detail as separate evidence.
#
# It fails unless (d) removed at least one build and every entry it removed is
# one (b) created. Set POOL_MAINTENANCE_EVIDENCE_DIR to keep the entry lists it
# compares (by metadata hash, before and after the timed pass) there.
# Quiet on success. Needs cargo, jq, just and the pinned pruner (`just bootstrap`).
set -euo pipefail

fail() {
  echo "measure.sh: $1" >&2
  echo "  next: $2" >&2
  exit 1
}

# Both inputs are paths read after the `cd` below, so each must be absolute; the
# evidence directory must not be an existing non-directory, and the result file's
# directory must exist (onebudgetspec creates it before running this command).
result="${ONEBUDGETSPEC_RESULT:-}"
if [ -n "$result" ]; then
  [[ "$result" == /* ]] || fail "ONEBUDGETSPEC_RESULT is not an absolute path: $result" \
    "run this through \`onebudgetspec check pool-maintenance/budgets.yaml\`, or set it to an absolute path"
  [ -d "$(dirname "$result")" ] || fail "ONEBUDGETSPEC_RESULT's directory does not exist: $result" \
    "create $(dirname "$result") or point ONEBUDGETSPEC_RESULT at an existing directory"
fi
work="${POOL_MAINTENANCE_EVIDENCE_DIR:-}"
if [ -n "$work" ]; then
  [[ "$work" == /* ]] || fail "POOL_MAINTENANCE_EVIDENCE_DIR is not an absolute path: $work" \
    "set it to an absolute directory path"
  [ ! -e "$work" ] || [ -d "$work" ] || fail "POOL_MAINTENANCE_EVIDENCE_DIR exists and is not a directory: $work" \
    "point it at a directory, or at a path that does not exist yet"
  mkdir -p "$work" || fail "cannot create POOL_MAINTENANCE_EVIDENCE_DIR $work" "check that its parent is writable"
else
  work="$(mktemp -d)" || fail "cannot create a scratch directory" "check that \$TMPDIR (or /tmp) is writable"
  trap 'rm -rf "$work" || echo "measure.sh: could not remove the scratch directory $work; delete it by hand" >&2' EXIT
fi

cd "$(dirname "$0")/.." || fail "cannot change to the repository root above $0" "run it from a complete checkout"

# Every top-level entry of each `deps/`, `.fingerprint/`, `build/` and
# `incremental/` directory under target/ (what the pruner may remove), sorted,
# written to $work/<name>.txt. Each directory is listed on its own, so one that
# cannot be read stops the measurement instead of reading as empty.
entries() {
  : >"$work/$1.txt" || fail "cannot write $work/$1.txt" "check that $work is writable"
  : >"$work/$1.unsorted" || fail "cannot write $work/$1.unsorted" "check that $work is writable"
  [ -d target ] || return 0
  find target \( -name deps -o -name .fingerprint -o -name build -o -name incremental \) \
    -type d -prune -print >"$work/$1.dirs" ||
    fail "cannot find target/'s build directories" "check that every directory under target/ is readable by this user"
  while IFS= read -r dir; do
    find "$dir" -mindepth 1 -maxdepth 1 >>"$work/$1.unsorted" ||
      fail "cannot list $dir" "check that it is readable by this user (\`chmod u+rx $dir\`), then rerun"
  done <"$work/$1.dirs" || fail "cannot read $work/$1.dirs" "check that $work is readable"
  LC_ALL=C sort "$work/$1.unsorted" >"$work/$1.txt" ||
    fail "cannot sort target/'s build entries into $work/$1.txt" "check that $work is writable"
}

# only_in <a> <b> <out>: the lines of $work/<a>.txt missing from $work/<b>.txt.
only_in() {
  LC_ALL=C comm -23 "$work/$1.txt" "$work/$2.txt" >"$work/$3.txt" ||
    fail "cannot compare $work/$1.txt with $work/$2.txt" "check that both lists exist and are sorted"
}

# The gate's build of the workspace's test targets (`just coverage` selects the
# same: the whole workspace with the fake provider). Extra arguments go to cargo.
build_tests() {
  cargo test --workspace --features fake-provider --no-run --locked --quiet "$@" >/dev/null
}

# (a) Clear any backlog, untimed; its report is kept apart from the figure.
ONEBUDGETSPEC_RESULT="$work/backlog.json" just pool-maintenance >/dev/null ||
  fail "the untimed backlog pass failed" "run \`just pool-maintenance\` to see why"

# (b) The alternative configuration's build, then backdate what it created.
entries before-alt
profile_args=()
packages="$(cargo metadata --no-deps --format-version 1 --locked | jq -r '.packages[].name')" ||
  fail "cannot list this workspace's packages" "run \`cargo metadata --no-deps --locked\` to see why"
while IFS= read -r package; do
  profile_args+=(--config "profile.dev.package.$package.debug=\"line-tables-only\"")
done <<<"$packages"
build_tests "${profile_args[@]}" ||
  fail "the alternative-profile build failed" "run \`cargo test --workspace --features fake-provider --no-run\` to see why"
entries after-alt
only_in after-alt before-alt alt
[ -s "$work/alt.txt" ] ||
  fail "the alternative-profile build created no new entries under target/" \
    "an interrupted earlier measurement leaves that build fresh: \`cargo clean -p\` this workspace's crates, then rerun"
# Backdated to 2000-01-01: past the 2-day window and older than every entry
# already there, so each unit's pre-existing ordinary build stays its newest one.
# `touch -h -t` is the same on GNU and BSD userlands.
while IFS= read -r entry; do
  find "$entry" -exec touch -h -t 200001010000 {} + ||
    fail "cannot backdate $entry" "check that target/ is writable by this user, then rerun"
done <"$work/alt.txt" || fail "cannot read $work/alt.txt" "check that $work is readable"

# (c) The ordinary gate build: each unit's newest build is now the ordinary one.
build_tests || fail "the ordinary build failed" "run \`cargo test --workspace --features fake-provider --no-run\` to see why"
entries before-timed
only_in alt before-timed missing
missing="$(cat "$work/missing.txt")" || fail "cannot read $work/missing.txt" "check that $work is readable"
[ -z "$missing" ] || fail "the ordinary build removed entries of the alternative build: $missing" \
  "rerun; the measurement needs both builds present before the timed pass"

# (d) The timed pass; the pruner reports its own seconds to ONEBUDGETSPEC_RESULT.
just pool-maintenance >/dev/null || fail "the timed pass failed" "run \`just pool-maintenance\` to see why"
entries after-timed
only_in before-timed after-timed removed
[ -s "$work/removed.txt" ] ||
  fail "the timed pass removed nothing, so it measured no superseded builds" \
    "check that the pruner still removes builds older than --keep-within with a newer build of the same unit"
only_in removed alt stray
stray="$(cat "$work/stray.txt")" || fail "cannot read $work/stray.txt" "check that $work is readable"
[ -z "$stray" ] || fail "the timed pass removed entries the alternative build did not create: $stray" \
  "the pruner removed an ordinary build; inspect $work (set POOL_MAINTENANCE_EVIDENCE_DIR to keep it)"

if [ -n "$result" ] && [ -f "$result" ] && [ -f "$work/backlog.json" ]; then
  backlog="$(jq -r '.value' "$work/backlog.json")" ||
    fail "cannot read the backlog pass's seconds from $work/backlog.json" "check that the pruner wrote a onebudgetspec result there"
  jq --arg backlog "$backlog" \
    '.detail = ((.detail // "") + "; untimed backlog pass seconds (not in the figure): " + $backlog)' \
    "$result" >"$work/result.json" ||
    fail "cannot add the backlog seconds to the timed pass's result $result" "check that it is the pruner's onebudgetspec JSON"
  cp "$work/result.json" "$result" || fail "cannot write the result back to $result" "check that its directory is writable"
fi
