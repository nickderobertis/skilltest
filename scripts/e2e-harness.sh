#!/usr/bin/env bash
#
# Live end-to-end check for ONE real harness: drive the built `skilltest` CLI
# against `<harness>` through `oneharness`, judged by a fixed claude-code judge,
# and assert the run passes and the skill's reply actually surfaced.
#
# This proves the whole skilltest pipeline against a real model — argument
# parsing, the oneharness provider, the conversation loop, the evals, the JSON
# contract, and the exit code — for the named harness. It is opt-in (not in
# `just check`): it needs the harness binary, oneharness, network, auth, and a
# cheap model call.
#
# Usage:
#   scripts/e2e-harness.sh <harness-id> [case-file]
#     harness-id: claude-code | opencode | goose | codex   (see scripts/e2e-lib.sh)
#     case-file:  defaults to tests/fixtures/live/cases/smoke.yaml
#
# Env overrides:
#   SKILLTEST_E2E_MODEL        model for the harness under test (per-harness default otherwise)
#   SKILLTEST_E2E_JUDGE_MODEL  model for the fixed judge (default: haiku)
#   SKILLTEST_BIN              path to a prebuilt skilltest (otherwise built here)
#
# Exit codes: 0 on pass OR skip (a harness that cannot run here is not a failure);
# non-zero only on a real assertion failure or a broken run.

set -euo pipefail

id="${1:-}"
case_file="${2:-}"
[ -n "$id" ] || { echo "usage: $0 <harness-id> [case-file]" >&2; exit 2; }

# shellcheck source=scripts/e2e-lib.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/e2e-lib.sh"

e2e_harness_config "$id"
e2e_preflight "$id"

e2e_run_smoke "$id" "$case_file"
e2e_assert_pass "$E2E_REPORT"

note "✓ $id live e2e passed"
