#!/usr/bin/env bash
#
# fake-claude.sh — a scripted stand-in for the `claude` CLI, used by the
# hermetic oneharness integration suite (tests/oneharness_integration.rs and
# the SDK integration tests) via `ONEHARNESS_BIN_CLAUDE_CODE=<this script>`.
#
# It speaks just enough of Claude Code's headless surface for the REAL
# oneharness binary to drive it exactly like the real thing — including the
# part no unit test can fake: it reads the per-run `--settings <file>`
# oneharness delivers for `run --mock-rules`/`--spy-file`, and **executes the
# installed PreToolUse hook** (`oneharness mock claude-code --rules … --spy-file …`)
# once per scripted tool call, applying the verdict the way claude-code does:
#
#   * no verdict          -> the call is "allowed"; nothing real runs — the
#                            result is the deterministic `SIM:<command>`.
#   * permissionDecision deny  -> the call never runs; the result is the
#                            model-visible denial reason.
#   * permissionDecision allow + updatedInput -> the input is substituted and
#                            the substituted command IS executed (it is
#                            oneharness's own safely-quoted printf stub), so
#                            the canned output round-trips for real. A
#                            non-command rewrite reports `REWRITTEN:<json>`.
#
# Scripted behavior comes from the skill instructions oneharness passes via
# `--append-system-prompt`, using the same markers the fake provider uses
# (tests/AGENTS.md): `fake-reply: <text>` is the reply; each
# `fake-tool: <name> <command>` is one tool call. A prompt asking for a JSON
# verdict (skilltest's judge prompt) gets `{"value": true, …}` so judge-backed
# evals can coexist with the deterministic ones — but note judge runs arrive
# WITHOUT `--settings` (skilltest never mocks the judge), which
# `JUDGE_SAW_SETTINGS` in the reply would expose if that ever regressed.
#
# Output is Claude Code's `-p --output-format stream-json --verbose` shape
# (what oneharness requests under `--events`): one assistant line per tool_use
# (post-rewrite input, like the real transcript), one user line per
# tool_result, then the terminal `{"type":"result",…}` carrying the reply text
# (with every tool result appended, standing in for "the model relayed what
# the tool returned"), a session id, and usage.

set -euo pipefail

prompt=""; settings=""; system=""
args=("$@")
i=0
while [ $i -lt ${#args[@]} ]; do
    case "${args[$i]}" in
        -p) prompt="${args[$((i + 1))]}"; i=$((i + 2)) ;;
        --settings) settings="${args[$((i + 1))]}"; i=$((i + 2)) ;;
        --append-system-prompt) system="${args[$((i + 1))]}"; i=$((i + 2)) ;;
        # Value-carrying flags we accept and ignore.
        --model | --permission-mode | --resume | --json-schema | --output-format)
            i=$((i + 2)) ;;
        *) i=$((i + 1)) ;;
    esac
done

# A judge/user-simulation run (no skill => no --append-system-prompt): reply
# with a passing verdict so judge-backed evals resolve deterministically.
if [ -z "$system" ]; then
    marker=""
    case "$prompt" in *"single-line JSON object"*) marker="verdict" ;; esac
    if [ -n "$settings" ]; then
        # skilltest must never deliver mocks to the judge; make it loud.
        text="JUDGE_SAW_SETTINGS"
    elif [ "$marker" = "verdict" ]; then
        text='{"value": true, "reason": "fake-claude judge"}'
    else
        text="continue"
    fi
    jq -cn --arg t "$text" \
        '{type:"result", result:$t, session_id:"fake-claude-judge", usage:{input_tokens:3, output_tokens:1}}'
    exit 0
fi

# The PreToolUse hook oneharness installed for this run, if any.
hook=""
if [ -n "$settings" ] && [ -f "$settings" ]; then
    hook="$(jq -r '.hooks.PreToolUse[0].hooks[0].command // empty' "$settings")"
fi

marker_text() { # marker_text <marker>: text after the marker, sans a trailing -->
    printf '%s\n' "$system" \
        | sed -n "s/.*$1[[:space:]]*//p" \
        | sed 's/[[:space:]]*-->.*$//' | sed 's/[[:space:]]*$//' | head -1
}
reply="$(marker_text 'fake-reply:')"
[ -n "$reply" ] || reply="ok"

surfaced=""
index=0

emit_call() { # emit_call <index> <name> <input-json> <output-text>
    jq -cn --arg n "$2" --argjson i "$3" --argjson idx "$1" \
        '{type:"assistant", message:{content:[{type:"tool_use", id:("toolu_" + ($idx|tostring)), name:$n, input:$i}]}}'
    jq -cn --arg o "$4" --argjson idx "$1" \
        '{type:"user", message:{content:[{type:"tool_result", tool_use_id:("toolu_" + ($idx|tostring)), content:$o}]}}'
}

# Iterate the scripted calls in a non-subshell loop so `surfaced` accumulates.
while IFS= read -r line; do
    rest="${line#*fake-tool:}"
    rest="$(printf '%s' "$rest" | sed 's/[[:space:]]*-->.*$//' | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')"
    name="${rest%% *}"
    command=""
    [ "$rest" != "$name" ] && command="${rest#* }"

    event="$(jq -cn --arg t "$name" --arg c "$command" \
        '{tool_name:$t, tool_input:{command:$c}, cwd:"/tmp", session_id:"fake-claude"}')"
    verdict=""
    if [ -n "$hook" ]; then
        # Exactly how claude-code invokes a command hook: the event JSON on
        # stdin, the command through the shell. The hook (oneharness mock)
        # appends to the spy log and prints a verdict or nothing.
        verdict="$(printf '%s' "$event" | sh -c "$hook" 2>/dev/null || true)"
    fi

    decision="$(jq -r '.hookSpecificOutput.permissionDecision // empty' <<<"$verdict" 2>/dev/null || true)"
    final_input="$(jq -cn --arg c "$command" '{command:$c}')"
    if [ "$decision" = "deny" ]; then
        reason="$(jq -r '.hookSpecificOutput.permissionDecisionReason // "denied"' <<<"$verdict")"
        output="$reason"
        surfaced="$surfaced"$'\n'"[$name] denied: $reason"
    elif [ "$decision" = "allow" ]; then
        final_input="$(jq -c '.hookSpecificOutput.updatedInput' <<<"$verdict")"
        substituted="$(jq -r '.command // empty' <<<"$final_input")"
        if [ -n "$substituted" ]; then
            # Execute the substituted command for real — it is oneharness's own
            # safely-quoted printf stub, so this is the canned output.
            output="$(sh -c "$substituted" 2>&1 || true)"
        else
            output="REWRITTEN:$final_input"
        fi
        surfaced="$surfaced"$'\n'"[$name] $output"
    else
        # Allowed through: nothing real executes in this shim.
        output="SIM:$command"
    fi

    emit_call "$index" "$name" "$final_input" "$output"
    index=$((index + 1))
done < <(printf '%s\n' "$system" | grep 'fake-tool:' || true)

jq -cn --arg t "$reply$surfaced" \
    '{type:"result", result:$t, session_id:"fake-claude-1", usage:{input_tokens:11, output_tokens:5}}'
