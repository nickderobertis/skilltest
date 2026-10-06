#!/usr/bin/env bash
# llmlint: ignore-file[new_code_lands_in_a_project] repo-level gate glue the justfile's affected tier calls, belonging to no Nx project (AGENTS.md: scripts/* are orchestrator-independent glue).
# Print the commit the affected tier diffs against, derived explicitly — never
# Nx's implicit `affected.defaultBase`, which is not deterministic across a
# shallow CI checkout.
#
#   * NX_BASE set (CI exports it with nrwl/nx-set-shas): it must be a plain git
#     ref name or SHA — letters, digits and `. _ / -` only, never a leading `-`,
#     so nothing a shell or git would read as an expansion or option reaches a
#     command — and it must resolve to a commit. Anything else (`HEAD~1`
#     included) fails closed, naming NX_BASE, rather than quietly gating less.
#   * NX_BASE unset or empty: the merge base of HEAD with origin/main.
#
# Prints the resolved commit SHA on stdout and nothing else; errors go to stderr
# with the fix.
set -euo pipefail

if [ -n "${NX_BASE:-}" ]; then
  if ! [[ "$NX_BASE" =~ ^[A-Za-z0-9._/][A-Za-z0-9._/-]*$ ]]; then
    echo "nx-base: NX_BASE must be a plain git ref or commit SHA (letters, digits and . _ / - only); got: $NX_BASE — unset it to diff against the merge base with origin/main" >&2
    exit 2
  fi
  if ! sha="$(git rev-parse --verify --quiet "${NX_BASE}^{commit}")"; then
    echo "nx-base: NX_BASE=$NX_BASE does not resolve to a commit in this clone; fetch it (git fetch origin) or unset NX_BASE to diff against the merge base with origin/main" >&2
    exit 2
  fi
  printf '%s\n' "$sha"
  exit 0
fi

if ! git rev-parse --verify --quiet "origin/main^{commit}" >/dev/null; then
  echo "nx-base: origin/main is not available to derive the merge base; run \`git fetch origin main\` (or set NX_BASE to a ref or SHA)" >&2
  exit 2
fi
if ! sha="$(git merge-base origin/main HEAD)"; then
  echo "nx-base: HEAD shares no history with origin/main; fetch full history (git fetch --unshallow) or set NX_BASE" >&2
  exit 2
fi
printf '%s\n' "$sha"
