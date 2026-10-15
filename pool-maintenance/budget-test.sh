#!/usr/bin/env bash
# Test of the `pool-maintenance` project's `budgets-host` target, run the way nx
# runs it — its own command, `onebudgetspec check` over budgets.yaml, which runs
# measure.sh, which runs the justfile's `pool-maintenance` recipe and the pinned
# pruner — against a small fixture instead of this workspace, so nothing here
# builds the workspace or prunes this checkout's target dir.
#
# The fixture is a one-crate cargo workspace carrying the committed justfile,
# `.cargo/config.toml`, measure.sh and budgets.yaml. Three cases: the real pruner
# passes the budget and reports builds removed; a failing pruner, and a pruner
# that removes nothing, each fail the check with measure.sh's own message.
#
# Quiet on success, one line. On failure it names the case and what happened.
# Needs cargo, jq, just, pnpm (`just bootstrap`) and the pinned pruner.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)" || {
  echo "budget-test: cannot locate the repository root above $0; run it from a complete checkout" >&2
  exit 1
}
work="$(mktemp -d)" || {
  echo "budget-test: could not create a scratch directory; check that \$TMPDIR (or /tmp) is writable" >&2
  exit 1
}
trap 'rm -rf "$work" || echo "budget-test: could not remove $work; delete it by hand" >&2' EXIT

fail() {
  echo "budget-test: $1 — fix pool-maintenance/measure.sh, budgets.yaml, project.json or the justfile's pool-maintenance recipe (or this test, if the rule changed on purpose)" >&2
  exit 1
}

for tool in cargo jq just pnpm cargo-prune-superseded; do
  command -v "$tool" >/dev/null || fail "needs \`$tool\` on PATH; run \`just bootstrap\`"
done

# The target's command exactly as nx resolves it from project.json.
command_line="$(cd "$root" && pnpm exec nx show project pool-maintenance --json)" ||
  fail "nx cannot show the pool-maintenance project"
command_line="$(jq -r '.targets["budgets-host"].options.command // empty' <<<"$command_line")" ||
  fail "cannot read the budgets-host command from nx's project JSON"
[ -n "$command_line" ] || fail "the pool-maintenance project has no budgets-host command"

# The fixture: the committed files the command path reads, over a one-crate
# workspace with the `fake-provider` feature measure.sh builds with.
repo="$work/repo"
mkdir -p "$repo/pool-maintenance" "$repo/.cargo" "$repo/src" || fail "cannot create the fixture under $work"
cp "$root/justfile" "$root/package.json" "$repo/" || fail "cannot copy the justfile and package.json into $repo"
cp "$root/.cargo/config.toml" "$repo/.cargo/" || fail "cannot copy .cargo/config.toml into $repo"
cp "$root/pool-maintenance/measure.sh" "$root/pool-maintenance/budgets.yaml" "$repo/pool-maintenance/" ||
  fail "cannot copy measure.sh and budgets.yaml into $repo/pool-maintenance"
ln -s "$root/node_modules" "$repo/node_modules" || fail "cannot link node_modules into the fixture $repo"
cat >"$repo/Cargo.toml" <<'EOF' || fail "cannot write the fixture's Cargo.toml"
[package]
name = "pool-fixture"
version = "0.0.0"
edition = "2021"
publish = false

[features]
fake-provider = []

[workspace]
EOF
cat >"$repo/src/lib.rs" <<'EOF' || fail "cannot write the fixture's src/lib.rs"
pub fn one() -> u8 {
    1
}

#[test]
fn one_is_one() {
    assert_eq!(one(), 1);
}
EOF
(cd "$repo" && cargo generate-lockfile --offline --quiet) || fail "cannot generate the fixture's Cargo.lock"

# check [stub dir]: runs the target's command in the fixture, with the stub dir
# (if any) ahead of PATH; sets $out (stdout and stderr) and $code.
check() {
  local path="$PATH"
  [ -z "${1:-}" ] || path="$1:$PATH"
  code=0
  (cd "$repo" && env -u CARGO_TARGET_DIR -u CARGO_BUILD_TARGET_DIR -u ONEBUDGETSPEC_RESULT \
    PATH="$path" bash -c "$command_line") >"$work/out" 2>&1 || code=$?
  out="$(cat "$work/out")" || fail "cannot read the captured output in $work/out"
}
# stub_pruner <dir> <script body>: a cargo-prune-superseded shadowing the real one.
stub_pruner() {
  mkdir -p "$1" || fail "cannot create the stub directory $1"
  printf '#!/bin/sh\n%s\n' "$2" >"$1/cargo-prune-superseded" || fail "cannot write the pruner stub into $1"
  chmod +x "$1/cargo-prune-superseded" || fail "cannot make the pruner stub in $1 executable"
}

# The real pruner: the budget passes, and its detail counts the builds the timed
# pass removed apart from the untimed backlog pass.
check
[ "$code" -eq 0 ] || fail "the budget over the fixture: expected exit 0, got exit $code: $out"
[[ "$out" == *"budget skilltest-pool-maintenance-seconds: actual "*" seconds, budget 20 seconds"*"— within"* ]] ||
  fail "the budget over the fixture: expected its within-budget line, got: $out"
removed="$(sed -n 's/.*builds removed: \([0-9][0-9]*\).*/\1/p' <<<"$out")" ||
  fail "cannot read the builds removed from the report: $out"
[ -n "$removed" ] && [ "$removed" -gt 0 ] || fail "the timed pass's detail reports no builds removed: $out"
[[ "$out" == *"untimed backlog pass seconds (not in the figure): "* ]] ||
  fail "the detail does not report the untimed backlog pass apart from the figure: $out"

# A failing pruner fails the backlog pass, and the check with it.
stub_pruner "$work/failing" 'echo "stub pruner failed" >&2; exit 3'
check "$work/failing"
[ "$code" -ne 0 ] && [[ "$out" == *"measure.sh: the untimed backlog pass failed"* ]] ||
  fail "a failing pruner: expected a non-zero exit naming the backlog pass, got exit $code: $out"

# A pruner that removes nothing measured no superseded builds, so it is refused.
stub_pruner "$work/idle" 'exit 0'
check "$work/idle"
[ "$code" -ne 0 ] && [[ "$out" == *"measure.sh: the timed pass removed nothing"* ]] ||
  fail "a pruner removing nothing: expected a non-zero exit naming the timed pass, got exit $code: $out"

# A build directory measure.sh cannot list stops it before it compares anything,
# rather than reading as an empty one. Release is a profile no case builds.
unreadable="$repo/target/release/incremental"
mkdir -p "$unreadable" || fail "cannot create $unreadable"
chmod 000 "$unreadable" || fail "cannot make $unreadable unreadable"
if ls "$unreadable" >/dev/null 2>&1; then
  fail "$unreadable stays readable after chmod 000 (running as root?); run this test as an unprivileged user"
fi
check "$work/idle"
chmod 755 "$unreadable" || fail "cannot restore the permissions of $unreadable"
rmdir "$unreadable" || fail "cannot remove $unreadable"
[ "$code" -ne 0 ] && [[ "$out" == *"measure.sh: cannot list target/release/incremental"* ]] ||
  fail "an unreadable build directory: expected a non-zero exit naming it, got exit $code: $out"

echo "budget-test: ok"
