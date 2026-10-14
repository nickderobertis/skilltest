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

cd "$(dirname "$0")/.."
result="${ONEBUDGETSPEC_RESULT:-}"
if [ -n "${POOL_MAINTENANCE_EVIDENCE_DIR:-}" ]; then
  work="$POOL_MAINTENANCE_EVIDENCE_DIR"
  mkdir -p "$work"
else
  work="$(mktemp -d)"
  trap 'rm -rf "$work"' EXIT
fi

fail() {
  echo "measure.sh: $1" >&2
  echo "  next: $2" >&2
  exit 1
}

# Every top-level entry of each `deps/`, `.fingerprint/`, `build/` and
# `incremental/` directory under target/ (what the pruner may remove), sorted.
entries() {
  [ -d target ] || return 0
  find target \( -name deps -o -name .fingerprint -o -name build -o -name incremental \) \
    -type d -prune -print0 |
    xargs -0 -r -I{} find {} -mindepth 1 -maxdepth 1 |
    LC_ALL=C sort
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
entries >"$work/before-alt.txt"
profile_args=()
for package in $(cargo metadata --no-deps --format-version 1 --locked | jq -r '.packages[].name'); do
  profile_args+=(--config "profile.dev.package.$package.debug=\"line-tables-only\"")
done
build_tests "${profile_args[@]}" ||
  fail "the alternative-profile build failed" "run \`cargo test --workspace --features fake-provider --no-run\` to see why"
entries >"$work/after-alt.txt"
LC_ALL=C comm -13 "$work/before-alt.txt" "$work/after-alt.txt" >"$work/alt.txt"
[ -s "$work/alt.txt" ] ||
  fail "the alternative-profile build created no new entries under target/" \
    "an interrupted earlier measurement leaves that build fresh: \`cargo clean -p\` this workspace's crates, then rerun"
# Older than the 2-day window and than every entry already there, so each unit's
# pre-existing ordinary build stays its newest one.
now="$(date +%s)"
stamp=$((now - 3 * 86400))
if [ -s "$work/before-alt.txt" ]; then
  oldest="$(xargs -d '\n' stat -c %Y <"$work/before-alt.txt" | sort -n | head -n 1)"
  [ "$oldest" -gt "$stamp" ] || stamp=$((oldest - 86400))
fi
while IFS= read -r entry; do
  find "$entry" -exec touch -h -d "@$stamp" {} +
done <"$work/alt.txt"

# (c) The ordinary gate build: each unit's newest build is now the ordinary one.
build_tests || fail "the ordinary build failed" "run \`cargo test --workspace --features fake-provider --no-run\` to see why"
entries >"$work/before-timed.txt"
missing="$(LC_ALL=C comm -23 "$work/alt.txt" "$work/before-timed.txt")"
[ -z "$missing" ] || fail "the ordinary build removed entries of the alternative build: $missing" \
  "rerun; the measurement needs both builds present before the timed pass"

# (d) The timed pass; the pruner reports its own seconds to ONEBUDGETSPEC_RESULT.
just pool-maintenance >/dev/null || fail "the timed pass failed" "run \`just pool-maintenance\` to see why"
entries >"$work/after-timed.txt"
LC_ALL=C comm -23 "$work/before-timed.txt" "$work/after-timed.txt" >"$work/removed.txt"
[ -s "$work/removed.txt" ] ||
  fail "the timed pass removed nothing, so it measured no superseded builds" \
    "check that the pruner still removes builds older than --keep-within with a newer build of the same unit"
stray="$(LC_ALL=C comm -23 "$work/removed.txt" "$work/alt.txt")"
[ -z "$stray" ] || fail "the timed pass removed entries the alternative build did not create: $stray" \
  "the pruner removed an ordinary build; inspect $work (set POOL_MAINTENANCE_EVIDENCE_DIR to keep it)"

if [ -n "$result" ] && [ -f "$result" ] && [ -f "$work/backlog.json" ]; then
  backlog="$(jq -r '.value' "$work/backlog.json")"
  jq --arg backlog "$backlog" \
    '.detail = ((.detail // "") + "; untimed backlog pass seconds (not in the figure): " + $backlog)' \
    "$result" >"$work/result.json"
  cp "$work/result.json" "$result"
fi
