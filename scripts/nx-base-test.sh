#!/usr/bin/env bash
# llmlint: ignore-file[new_code_lands_in_a_project] tests scripts/nx-base.sh, repo-level gate glue that belongs to no Nx project; run workspace-wide from `just check` (AGENTS.md: scripts/* are orchestrator-independent glue).
# Test of scripts/nx-base.sh, the affected tier's base derivation, driven as a
# subprocess against throwaway git repositories: every way NX_BASE can resolve,
# and every way it must fail closed, since a base that silently resolves wrong
# makes `just check` gate less than the change reaches.
#
# Quiet on success, one line. On failure it names the case and what the script did.
set -euo pipefail

script="$(cd "$(dirname "$0")" && pwd)/nx-base.sh" || {
  echo "nx-base-test: cannot locate scripts/nx-base.sh beside $0; run it from a complete checkout" >&2
  exit 1
}
work="$(mktemp -d)" || {
  echo "nx-base-test: could not create a scratch directory; check that \$TMPDIR (or /tmp) is writable" >&2
  exit 1
}
trap 'rm -rf "$work" || echo "nx-base-test: could not remove $work; delete it by hand" >&2' EXIT

fail() {
  echo "nx-base-test: $1 — fix scripts/nx-base.sh (or this test, if the rule changed on purpose)" >&2
  exit 1
}

# Run one git setup step quietly; on failure, say which step and what git said.
git_q() {
  local said
  said="$(git -c user.name=t -c user.email=t@t -c init.defaultBranch=main "$@" 2>&1)" || {
    echo "nx-base-test: setup step \`git $*\` failed: $said — check that git works and \$TMPDIR is writable" >&2
    exit 1
  }
}
rev() { git -C "$repo" rev-parse "$1" 2>/dev/null || fail "setup could not resolve $1 in the scratch repository"; }

# A repository whose `main` (mirrored as origin/main) forks into a branch that
# is checked out, so the merge base differs from both tips.
repo="$work/repo"
git_q init "$repo"
git_q -C "$repo" commit --allow-empty -m base
git_q -C "$repo" commit --allow-empty -m on-main
git_q -C "$repo" checkout -b feature HEAD~1
git_q -C "$repo" commit --allow-empty -m on-feature
git_q -C "$repo" update-ref refs/remotes/origin/main main
fork_point="$(rev HEAD~1)"
main_tip="$(rev main)"

# run <NX_BASE or "-" for unset>: sets $out, $err and $code.
run() {
  if [ "$1" = "-" ]; then
    code=0; (cd "$repo" && env -u NX_BASE bash "$script") >"$work/out" 2>"$work/err" || code=$?
  else
    code=0; (cd "$repo" && NX_BASE="$1" bash "$script") >"$work/out" 2>"$work/err" || code=$?
  fi
  out="$(cat "$work/out")" || fail "cannot read the captured stdout in $work"
  err="$(cat "$work/err")" || fail "cannot read the captured stderr in $work"
}

expect_sha() { # label NX_BASE expected-sha
  run "$2"
  [ "$code" -eq 0 ] && [ "$out" = "$3" ] || fail "$1: expected exit 0 printing $3, got exit $code, stdout '$out', stderr '$err'"
}
expect_refusal() { # label NX_BASE stderr-needle
  run "$2"
  [ "$code" -eq 2 ] && [ -z "$out" ] && [[ "$err" == *"$3"* ]] ||
    fail "$1: expected exit 2 naming '$3' with nothing on stdout, got exit $code, stdout '$out', stderr '$err'"
}

expect_sha "unset: the merge base with origin/main" - "$fork_point"
expect_sha "empty: treated as unset" "" "$fork_point"
expect_sha "a plain ref name" main "$main_tip"
expect_sha "a ref name with a slash" origin/main "$main_tip"
expect_sha "a full commit SHA" "$fork_point" "$fork_point"
expect_sha "an abbreviated commit SHA" "${fork_point:0:10}" "$fork_point"
expect_refusal "shell metacharacters" 'main;rm -rf ~' "NX_BASE must be a plain git ref"
expect_refusal "a command substitution" "\$(id)" "NX_BASE must be a plain git ref"
expect_refusal "a leading dash (an option to git)" "--all" "NX_BASE must be a plain git ref"
expect_refusal "a revision expression, not a plain ref" "HEAD~1" "NX_BASE must be a plain git ref"
expect_refusal "a ref that does not resolve" "no-such-branch" "NX_BASE=no-such-branch does not resolve"

git_q -C "$repo" update-ref -d refs/remotes/origin/main
expect_refusal "unset with no origin/main" - "origin/main is not available"

git_q -C "$repo" checkout --orphan unrelated
git_q -C "$repo" commit --allow-empty -m unrelated
git_q -C "$repo" update-ref refs/remotes/origin/main main
expect_refusal "unset with no shared history" - "shares no history with origin/main"

echo "nx-base-test: 6 resolving and 7 refusing cases behave"
