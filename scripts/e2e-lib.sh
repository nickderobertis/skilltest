# shellcheck shell=bash
# Shared helpers for the live, per-harness e2e checks (scripts/e2e-harness.sh).
# This file is sourced (no shebang); the directive above tells shellcheck the
# target shell.
#
# These drive the *built* `skilltest` CLI as a subprocess — exactly how a user
# does — against a real harness through `oneharness`, then assert on the JSON
# report. They are deliberately NOT in `just check`: they need a harness binary,
# network, auth, and a (cheap) model call, so they are neither hermetic nor
# deterministic the way the `tests/` suite is.
#
# Design, mirroring nickderobertis/allowlister's live scripts:
#   * A missing oneharness / harness binary / auth secret is a SKIP, not a
#     failure — the rest of the project must build and test without them.
#   * A harness that `oneharness` cannot yet carry the skill to is a SKIP with a
#     precise reason, never a false pass (see the per-harness table below).
#   * The judge is a FIXED harness (claude-code), independent of the harness under
#     test — this is skilltest's own model ("evals run on a fixed judge_harness")
#     and keeps verdict parsing reliable even when the harness under test wraps
#     its output in banners/tool noise.
#
# Sourced AFTER the calling script defines nothing — `note`/`fail`/`skip` live
# here so every harness check stays consistent.

note() { printf '%s\n' "$*"; }
fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }
# A skip is a non-failure: print why and exit 0 so CI stays green when a harness
# simply isn't available here.
skip() { printf 'SKIP: %s\n' "$*"; exit 0; }

# The fixed judge. claude-code + haiku gives clean, reliably-parsed verdicts
# regardless of which harness produced the skill's reply, so every harness check
# also needs CLAUDE_CODE_OAUTH_TOKEN (for claude-code itself it is the same key).
E2E_JUDGE_PLATFORM="claude-code"
E2E_JUDGE_MODEL="${SKILLTEST_E2E_JUDGE_MODEL:-haiku}"

# Repo root (this file lives in scripts/).
e2e_repo_root() { cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd; }

# Resolve (building if needed) the skilltest binary. Honor SKILLTEST_BIN like the
# plugin suites do, so CI can point at a prebuilt one and skip the rebuild.
e2e_skilltest_bin() {
    local root; root="$(e2e_repo_root)"
    if [ -n "${SKILLTEST_BIN:-}" ]; then printf '%s' "$SKILLTEST_BIN"; return; fi
    local bin="$root/target/debug/skilltest"
    if [ ! -x "$bin" ]; then
        ( cd "$root" && cargo build -p skilltest-cli --quiet ) >&2 || fail "could not build skilltest"
    fi
    printf '%s' "$bin"
}

# Per-harness configuration. Sets these globals for $1 (a oneharness harness id):
#   H_PLATFORM   oneharness harness id passed to `--platform`
#   H_BIN        the harness CLI binary that must be on PATH
#   H_MODEL      model passed to `--model` (override with SKILLTEST_E2E_MODEL)
#   H_AUTH_ENV   the env var that must hold the harness's credential
#   H_EXTRA_ENV  extra KEY=VALUE env the harness needs (space separated; may be empty)
#   H_DRIVABLE   1 if the installed oneharness can deliver the skill to this
#                harness, else 0
#   H_BLOCKED    when H_DRIVABLE=0, the precise upstream reason (shown on SKIP)
#
# Why H_DRIVABLE exists: skilltest always passes the skill as `--system`. With
# oneharness v0.2.0 only claude-code maps that to a real system prompt; for every
# other harness oneharness forwards the system text as a positional argument,
# which the harness CLI rejects ("unexpected argument '---\nname: …'"). So those
# harnesses cannot be driven yet — flip H_DRIVABLE to 1 (and drop H_BLOCKED) once
# oneharness gains real `--system` support for them. See docs/e2e.md.
e2e_harness_config() {
    local id="$1"
    H_EXTRA_ENV=""; H_BLOCKED=""
    case "$id" in
        claude-code)
            H_PLATFORM="claude-code"; H_BIN="claude"
            H_MODEL="${SKILLTEST_E2E_MODEL:-haiku}"
            H_AUTH_ENV="CLAUDE_CODE_OAUTH_TOKEN"; H_DRIVABLE=1 ;;
        opencode)
            H_PLATFORM="opencode"; H_BIN="opencode"
            H_MODEL="${SKILLTEST_E2E_MODEL:-openai/gpt-5-mini}"
            H_AUTH_ENV="OPENAI_API_KEY"; H_DRIVABLE=0
            H_BLOCKED="oneharness $(_e2e_oh_version) forwards skilltest's --system as a positional arg, which opencode rejects" ;;
        goose)
            H_PLATFORM="goose"; H_BIN="goose"
            H_MODEL="${SKILLTEST_E2E_MODEL:-gpt-5-mini}"
            H_AUTH_ENV="OPENAI_API_KEY"; H_EXTRA_ENV="GOOSE_PROVIDER=openai"; H_DRIVABLE=0
            H_BLOCKED="oneharness $(_e2e_oh_version) forwards skilltest's --system as a positional arg, which goose rejects" ;;
        codex)
            H_PLATFORM="codex"; H_BIN="codex"
            H_MODEL="${SKILLTEST_E2E_MODEL:-gpt-5-mini}"
            H_AUTH_ENV="OPENAI_API_KEY"; H_DRIVABLE=0
            H_BLOCKED="oneharness $(_e2e_oh_version) invokes 'codex exec -a never'; codex-cli >=0.135 removed -a" ;;
        *)
            fail "unknown harness id '$id' (known: claude-code, opencode, goose, codex)" ;;
    esac
}

# Best-effort oneharness version string for the SKIP reason (or "0.2.x").
_e2e_oh_version() { oneharness --version 2>/dev/null | awk '{print $2}' || true; }

# Run the preflight skip checks for harness id $1. Exits 0 (skip) when the check
# cannot run here; returns 0 when it can.
e2e_preflight() {
    local id="$1"
    command -v oneharness >/dev/null 2>&1 \
        || skip "\`oneharness\` not on PATH — install it: scripts/install-oneharness.sh"
    command -v jq >/dev/null 2>&1 || fail "this check needs \`jq\` to read the JSON report"
    command -v "$H_BIN" >/dev/null 2>&1 \
        || skip "\`$H_BIN\` (the $id CLI) not on PATH — install it to run this check"
    if [ "$H_DRIVABLE" -ne 1 ]; then
        skip "$id cannot be driven by the installed oneharness yet: $H_BLOCKED"
    fi
    [ -n "${!H_AUTH_ENV:-}" ] \
        || skip "\$$H_AUTH_ENV is not set — needed to authenticate $id (sync it: gh-secrets manifest sync)"
    [ -n "${CLAUDE_CODE_OAUTH_TOKEN:-}" ] \
        || skip "\$CLAUDE_CODE_OAUTH_TOKEN is not set — needed for the fixed $E2E_JUDGE_PLATFORM judge"
}

# Drive one live case through the built CLI. Sets E2E_REPORT to the JSON report
# path for e2e_assert_pass to read (a global, not stdout — this function prints
# human progress, which a command substitution would otherwise swallow).
# Args: <harness-id> [case-file]   (case defaults to the harness-agnostic smoke)
e2e_run_smoke() {
    local id="$1"
    local root; root="$(e2e_repo_root)"
    local case_file="${2:-$root/tests/fixtures/live/cases/smoke.yaml}"
    local bin; bin="$(e2e_skilltest_bin)"
    local out; out="$(mktemp)"
    # Harness-specific env (e.g. GOOSE_PROVIDER=openai) for this invocation only.
    local kv
    for kv in $H_EXTRA_ENV; do export "${kv?}"; done
    note "» running smoke on $id (model=$H_MODEL, judge=$E2E_JUDGE_PLATFORM/$E2E_JUDGE_MODEL)"
    local code=0
    "$bin" run "$case_file" \
        --oneharness-bin oneharness \
        --platform "$H_PLATFORM" --model "$H_MODEL" \
        --judge-harness "$E2E_JUDGE_PLATFORM" --judge-model "$E2E_JUDGE_MODEL" \
        --timeout 150 --format json >"$out" 2>"$out.err" || code=$?
    # exit 0 == pass, exit 1 == ran but an eval failed: both are assertion
    # outcomes the caller judges from the report. Any other code is a real error.
    if [ "$code" -ne 0 ] && [ "$code" -ne 1 ]; then
        note "  stderr:"; sed 's/^/    /' "$out.err" >&2 || true
        fail "skilltest run errored (exit $code) for $id — see stderr above"
    fi
    # shellcheck disable=SC2034  # consumed by e2e-harness.sh after this returns
    E2E_REPORT="$out"
}

# Assert a report shows an overall pass and the reply actually contained "pong".
# Args: <report-json-path>
e2e_assert_pass() {
    local report="$1"
    jq -e '.passed == true' "$report" >/dev/null 2>&1 \
        || { note "  report:"; jq '{passed, evals:[.runs[0].evals[]?|{passed,detail:.detail.kind}]}' "$report" 2>/dev/null | sed 's/^/    /'; fail "the live run did not pass"; }
    jq -e '[.runs[0].transcript.messages[]?|select(.role=="assistant")|.content]|join(" ")|ascii_downcase|contains("pong")' "$report" >/dev/null 2>&1 \
        || fail "the assistant reply never contained \"pong\" (the harness may not have applied the skill)"
    note "  ok: live run passed and the reply contained \"pong\""
}
