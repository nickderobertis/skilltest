#!/usr/bin/env bash
# Test of this repository's declared pool maintenance, driven the way onevcs and
# a host's bootstrap drive it: the `command` onevcs.toml declares, run from the
# repository root as a subprocess; the justfile's `pool-maintenance-tool` install;
# and the `budgets` recipe's task selection against `check`'s. Only the external
# binaries are doubled — cargo-prune-superseded and cargo, as recording stubs on a
# PATH that holds nothing else but `just` and the shells — so no build runs, no
# target dir is pruned and nothing is installed.
#
# It is the drift gate between onevcs.toml, the recipe and `.cargo/config.toml`:
# the declared command must reach the pruner with `--keep-within 2d` and exactly
# the target dirs the committed cargo configuration resolves to.
#
# Quiet on success, one line. On failure it names the case and what happened.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)" || {
  echo "pool-maintenance-test: cannot locate the repository root above $0; run it from a complete checkout" >&2
  exit 1
}
work="$(mktemp -d)" || {
  echo "pool-maintenance-test: could not create a scratch directory; check that \$TMPDIR (or /tmp) is writable" >&2
  exit 1
}
trap 'rm -rf "$work" || echo "pool-maintenance-test: could not remove $work; delete it by hand" >&2' EXIT

fail() {
  echo "pool-maintenance-test: $1 — fix onevcs.toml, the justfile's pool-maintenance recipes or pool-maintenance/ (or this test, if the rule changed on purpose)" >&2
  exit 1
}

for tool in just bash sh python3 pnpm jq; do
  command -v "$tool" >/dev/null || fail "needs \`$tool\` on PATH; run \`just bootstrap\`"
done

# A PATH holding only `just`, the shells and whichever stubs a case installs.
bin="$work/bin"
mkdir -p "$bin" || fail "cannot create the sandbox PATH directory $bin; check that $work is writable"
for tool in just bash sh; do
  ln -s "$(command -v "$tool")" "$bin/$tool" || fail "cannot link $tool into the sandbox PATH $bin; check that $work is writable"
done
calls="$work/calls"
bash_path="$(command -v bash)"
# stub <name> <exit code> [line printed for --version]: records each call's argv.
stub() {
  {
    echo "#!$bash_path"
    echo "echo \"\$0 \$*\" >>'$calls'"
    [ -z "${3:-}" ] || echo "[ \"\${1:-}\" != --version ] || { echo '$3'; exit 0; }"
    echo "exit $2"
  } >"$bin/$1" || fail "cannot write the $1 stub into $bin; check that $work is writable"
  chmod +x "$bin/$1" || fail "cannot make the $1 stub executable in $bin"
}
unstub() {
  rm -f "$bin/cargo-prune-superseded" "$bin/cargo" "$calls" || fail "cannot remove the stubs from $bin; check that $work is writable"
}

# run <argv...>: from the repository root on the sandbox PATH; sets $err and $code.
run() {
  code=0
  (cd "$root" && env PATH="$bin" "$@") >/dev/null 2>"$work/err" || code=$?
  err="$(cat "$work/err")" || fail "cannot read the captured stderr in $work"
  recorded
}
# recorded: sets $got to the calls the stubs recorded ("" when none ran) and
# $installs to how many of them were `cargo install`.
recorded() {
  got=""
  installs=0
  [ -f "$calls" ] || return 0
  got="$(cat "$calls")" || fail "cannot read the recorded calls in $calls"
  installs="$(grep -c '/cargo install' <<<"$got")" || [ "$installs" = 0 ] ||
    fail "cannot count the recorded \`cargo install\` calls in $calls"
}
# matches <pattern> <text>: grep -q, but a grep error fails the test rather than
# reading as "no match".
matches() {
  local status=0
  grep -q "$1" <<<"$2" || status=$?
  [ "$status" -le 1 ] || fail "grep could not search for '$1'"
  return "$status"
}

# The command onevcs.toml declares, and the target dirs the committed
# `.cargo/config.toml` files resolve to (relative to the directory holding
# `.cargo/`, cargo's rule), repository-relative; `target` where none sets one.
declared="$(python3 -I - "$root" <<'PY'
import pathlib, subprocess, sys, tomllib

root = pathlib.Path(sys.argv[1])
config = tomllib.loads((root / "onevcs.toml").read_text())
print("\x1f".join(config["pool"]["maintain"]["command"]))
listed = subprocess.run(
    ["git", "-C", str(root), "ls-files", "--", ".cargo/config.toml", "**/.cargo/config.toml"],
    check=True, capture_output=True, text=True,
).stdout.split()
dirs = []
for rel in listed:
    holder = (root / rel).parent.parent
    target = tomllib.loads((root / rel).read_text()).get("build", {}).get("target-dir")
    if target is not None:
        dirs.append(str(((holder / target).resolve()).relative_to(root.resolve())))
print(" ".join(sorted(set(dirs)) or ["target"]))
PY
)" || fail "cannot read onevcs.toml's [pool.maintain] command or the .cargo/config.toml target dirs"
command_line="${declared%%$'\n'*}"
target_dirs="${declared#*$'\n'}"
[ "$command_line" != "$declared" ] || fail "reading onevcs.toml and .cargo/config.toml printed one line, not the command and the target dirs"
IFS=$'\x1f' read -r -a command <<<"$command_line"
[ "${#command[@]}" -gt 0 ] || fail "onevcs.toml's [pool.maintain] command is empty"

# The declared command reaches the pruner with the window and every target dir.
unstub
stub cargo-prune-superseded 0
run "${command[@]}"
want="$bin/cargo-prune-superseded --keep-within 2d $target_dirs"
[ "$code" -eq 0 ] && [ "$got" = "$want" ] ||
  fail "the declared command (${command[*]}): expected exit 0 and one call '$want', got exit $code, calls '$got', stderr '$err'"

# A failing pass fails the declared command, so onevcs sees it.
unstub
stub cargo-prune-superseded 3
run "${command[@]}"
[ "$code" -ne 0 ] || fail "the declared command exited 0 although the pruner exited 3"

# Without the pruner it refuses, naming the bootstrap that installs it.
unstub
run "${command[@]}"
[ "$code" -ne 0 ] && [[ "$err" == *"cargo-prune-superseded not found"*"just bootstrap"* ]] ||
  fail "the declared command without the pruner: expected a refusal naming \`just bootstrap\`, got exit $code, stderr '$err'"

# The install: the pinned release when absent or stale, nothing when current.
pin="$(cd "$root" && just --evaluate prune-superseded-version)" || fail "cannot evaluate the justfile's prune-superseded-version"
[ -n "$pin" ] || fail "the justfile's prune-superseded-version is empty"
want_install="$bin/cargo install --locked cargo-prune-superseded --version $pin"
unstub
stub cargo 0
run just pool-maintenance-tool
[ "$code" -eq 0 ] && [ "$got" = "$want_install" ] ||
  fail "pool-maintenance-tool without the pruner: expected '$want_install', got exit $code, calls '$got', stderr '$err'"
unstub
stub cargo 0
stub cargo-prune-superseded 0 "cargo-prune-superseded 0.0.0-stale"
run just pool-maintenance-tool
[ "$code" -eq 0 ] && [ "$installs" = 1 ] ||
  fail "pool-maintenance-tool over a stale pruner: expected one install, got exit $code, calls '$got', stderr '$err'"
unstub
stub cargo 0
stub cargo-prune-superseded 0 "cargo-prune-superseded $pin"
run just pool-maintenance-tool
if [ "$code" -ne 0 ] || [ "$installs" != 0 ]; then
  fail "pool-maintenance-tool over the pinned release: expected no install, got exit $code, calls '$got'"
fi
unstub
stub cargo 101
run just pool-maintenance-tool
[ "$code" -ne 0 ] || fail "pool-maintenance-tool exited 0 although cargo install failed"

# Task selection: the `budgets` recipe runs the measurement's budget check, while
# `check` — over every project, and affected by a change to the maintenance
# files, to the project's configuration or to a source file — never does.
nx_line() { # recipe tier: the recipe's `nx run-many` line, as just would run it
  local plan
  plan="$(cd "$root" && just --dry-run "$1" "$2" 2>&1)" || fail "\`just --dry-run $1 $2\` failed: $plan"
  local line status=0
  line="$(grep -m 1 '^pnpm exec nx run-many -t ' <<<"$plan")" || status=$?
  [ "$status" -le 1 ] || fail "grep could not read the plan of \`just --dry-run $1 $2\`"
  printf '%s\n' "$line"
}
tasks() { # nx argv...: the task ids nx would run
  (cd "$root" && "$@" --graph="$work/graph.json" >/dev/null 2>&1) || fail "nx could not plan \`$*\`"
  jq -r '.tasks.tasks | keys[]' "$work/graph.json" || fail "cannot read the task ids from nx's graph $work/graph.json for \`$*\`"
}
measured="pool-maintenance:budgets-host"
budgets_line="$(nx_line budgets all)" || exit 1
check_line="$(nx_line check all)" || exit 1
[ -n "$budgets_line" ] && [ -n "$check_line" ] || fail "cannot read the nx lines of \`just budgets all\` / \`just check all\`"
read -r -a budgets_argv <<<"$budgets_line"
read -r -a check_argv <<<"$check_line"
selected="$(tasks "${budgets_argv[@]}")" || exit 1
matches "^$measured\$" "$selected" || fail "\`just budgets all\` does not select $measured"
selected="$(tasks "${check_argv[@]}")" || exit 1
! matches '^pool-maintenance:budgets' "$selected" || fail "\`just check all\` selects the pool-maintenance budget"
affected_argv=("${check_argv[@]/run-many/affected}")
for changed in pool-maintenance/measure.sh pool-maintenance/budgets.yaml pool-maintenance/project.json \
  pool-maintenance/declaration/pool-maintenance-test.sh pool-maintenance/declaration/project.json \
  nx.json crates/skilltest-core/src/lib.rs; do
  selected="$(tasks "${affected_argv[@]}" --files="$changed")" || exit 1
  ! matches '^pool-maintenance:budgets' "$selected" ||
    fail "\`just check\` affected by $changed selects the pool-maintenance budget"
done

echo "pool-maintenance-test: ok"
