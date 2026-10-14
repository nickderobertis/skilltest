#!/usr/bin/env bash
# llmlint: ignore-file[new_code_lands_in_a_project] A paid-boundary double shared like fake-claude.sh and fake-goose.sh: nx.json sharedGlobals hashes tests/fixtures for the projects whose tests run it, and it has no runtime or target of its own.
# llmlint: ignore-file[contracts_have_one_source_or_a_drift_gate] The codex CLI's argv, hooks.json, hook event and `exec --json` records belong to codex and oneharness, which publish no schema to derive this double from. Their drift alarms are the real oneharness binary, which writes the hooks.json and parses every record here in `just test-oneharness` (CI's e2e-claude workflow), and the live codex suite, which runs the same mock phase against real codex.
#
# fake-codex.sh — a scripted stand-in for the `codex` CLI, used by the hermetic
# oneharness integration suite (tests/oneharness_integration.rs) via
# `ONEHARNESS_BIN_CODEX=<this script>`, so the live mock phase
# (`e2e_mock_phase` in scripts/e2e-lib.sh) can run with no model call.
#
# It plays gpt-5-mini following the live `toolrunner` skill, which oneharness
# prepends to the `codex exec` prompt: it runs the skill's one indented shell
# command, and **executes the PreToolUse hook** oneharness installs for
# `run --mock-rules`/`--spy-file` in the project's `.codex/hooks.json`
# (`oneharness mock codex --rules … --spy-file …`), applying the verdict the
# way codex does:
#
#   * no verdict          -> the command runs for real.
#   * permissionDecision deny  -> the command never runs; the result is the
#                            model-visible denial reason.
#   * permissionDecision allow + updatedInput -> the substituted command runs
#                            (oneharness's own safely-quoted printf stub), so
#                            the canned output round-trips for real.
#
# FAKE_CODEX_SCENARIO picks how the "model" behaves, one per way the live mock
# phase must go red or green:
#
#   relay    (default) the reply quotes what the tool returned.
#   no-hook  the hook never fires (as if codex ignored hooks.json); the reply
#            quotes the real output.
#   ignore   the hook fires, but the reply never mentions the tool's output.
#   guess    the hook fires, but the reply reports the live output, MARKER-LIVE.
#
# Output is `codex exec --json`'s JSONL (what oneharness reads under
# `--events`): thread.started, one command_execution item started/completed
# carrying the post-hook command and its aggregated_output, the agent_message
# reply, and turn.completed with usage.

set -euo pipefail

command -v jq >/dev/null 2>&1 || {
    echo "fake-codex: jq is not on PATH, and every transcript record is written with it; install jq and rerun the oneharness integration tests" >&2
    exit 2
}

emit() { # emit <jq args…> — one transcript record, or stop the run loudly
    # llmlint: ignore[tool_output_is_signal] This stdout record is the `codex exec --json` protocol consumed by real oneharness; each transcript event requires its own JSON record.
    jq -cn "$@" || {
        echo "fake-codex: jq could not write a transcript record (see its error above); fix the record's arguments and rerun" >&2
        exit 1
    }
}

# `codex exec [flags] <prompt>`: exactly one prompt, after the `exec` subcommand.
# Only the flags oneharness passes codex are accepted, so a new one is loud.
args=("$@")
if [ "${#args[@]}" -eq 0 ] || [ "${args[0]}" != "exec" ]; then
    echo "fake-codex: expected 'exec' as the first argument (oneharness drives \`codex exec\`); got '${args[0]:-}'. Check the oneharness codex invocation and rerun" >&2
    exit 2
fi
prompt=""
prompts=0
i=1
while [ "$i" -lt "${#args[@]}" ]; do
    case "${args[$i]}" in
        --json | --dangerously-bypass-approvals-and-sandbox | --dangerously-bypass-hook-trust | --full-auto | --skip-git-repo-check)
            i=$((i + 1)) ;;
        -c | --config | -m | --model | -s | --sandbox | --output-schema)
            if [ "$((i + 1))" -ge "${#args[@]}" ]; then
                echo "fake-codex: ${args[$i]} requires a value; pass ${args[$i]} <value> and rerun the integration test" >&2
                exit 2
            fi
            i=$((i + 2)) ;;
        -*)
            echo "fake-codex: unsupported flag '${args[$i]}'; teach this shim what codex does with it, then rerun the integration test" >&2
            exit 2 ;;
        *) prompt="${args[$i]}"; prompts=$((prompts + 1)); i=$((i + 1)) ;;
    esac
done
if [ "$prompts" -ne 1 ]; then
    echo "fake-codex: expected exactly one prompt argument after exec, got $prompts; check the oneharness codex invocation and rerun" >&2
    exit 2
fi
if [ "$prompt" = "-" ]; then
    prompt="$(cat)" || {
        echo "fake-codex: could not read the prompt from stdin; pass it as the exec argument and rerun" >&2
        exit 2
    }
fi

scenario="${FAKE_CODEX_SCENARIO:-relay}"
case "$scenario" in
    relay | no-hook | ignore | guess) ;;
    *)
        echo "fake-codex: unknown FAKE_CODEX_SCENARIO '$scenario'; use relay, no-hook, ignore or guess" >&2
        exit 2 ;;
esac

# The command the skill asks for is its first line indented by four spaces.
command=""
while IFS= read -r line; do
    if [[ "$line" =~ ^\ \ \ \ ([^[:space:]].*[^[:space:]]|[^[:space:]])[[:space:]]*$ ]]; then
        command="${BASH_REMATCH[1]}"
        break
    fi
done <<<"$prompt"
if [ -z "$command" ]; then
    echo "fake-codex: the prompt carries no four-space-indented command to run; give the skill one (as tests/fixtures/live/skills/toolrunner does)" >&2
    exit 2
fi

# oneharness installs the run's PreToolUse hook into the project's hooks.json.
hook=""
if [ "$scenario" != "no-hook" ] && [ -e .codex/hooks.json ]; then
    hook="$(jq -er '.hooks.PreToolUse[0].hooks[0].command | strings' .codex/hooks.json)" || {
        echo "fake-codex: .codex/hooks.json has no string PreToolUse command (or is unreadable); check what oneharness installed for --mock-rules and rerun" >&2
        exit 2
    }
fi

verdict=""
if [ -n "$hook" ]; then
    # Exactly how codex invokes a command hook: the event JSON on stdin, the
    # command through the shell. The hook (oneharness mock) appends to the spy
    # log and prints a verdict or nothing.
    event="$(jq -cn --arg c "$command" --arg cwd "$PWD" \
        '{hook_event_name:"PreToolUse", tool_name:"Bash", tool_input:{command:$c}, cwd:$cwd, session_id:"fake-codex"}')" || {
        echo "fake-codex: jq could not build the hook event (see its error above); fix the event's arguments and rerun" >&2
        exit 1
    }
    verdict="$(printf '%s' "$event" | sh -c "$hook")" || {
        echo "fake-codex: the PreToolUse hook '$hook' failed (see its error above); fix the mock rules or the oneharness install and rerun" >&2
        exit 1
    }
fi

decision=""
if [ -n "$verdict" ]; then
    decision="$(jq -er '.hookSpecificOutput.permissionDecision | select(. == "allow" or . == "deny")' <<<"$verdict")" || {
        echo "fake-codex: the hook printed a verdict without an allow/deny permissionDecision: $verdict; check the oneharness mock codex protocol and rerun" >&2
        exit 1
    }
fi
ran="$command"
if [ "$decision" = "deny" ]; then
    output="$(jq -er '.hookSpecificOutput.permissionDecisionReason | strings' <<<"$verdict")" || {
        echo "fake-codex: the deny verdict carries no string permissionDecisionReason: $verdict; check the oneharness mock codex protocol and rerun" >&2
        exit 1
    }
    code=1
else
    if [ "$decision" = "allow" ]; then
        ran="$(jq -er '.hookSpecificOutput.updatedInput.command | strings' <<<"$verdict")" || {
            echo "fake-codex: the allow verdict carries no string updatedInput.command: $verdict; check the oneharness mock codex protocol and rerun" >&2
            exit 1
        }
    fi
    code=0
    output="$(sh -c "$ran" 2>&1)" || code=$?
fi

case "$scenario" in
    relay | no-hook) reply="The command printed: $output" ;;
    ignore) reply="I ran the command as instructed." ;;
    guess) reply="The command printed: MARKER-LIVE" ;;
esac

# llmlint: ignore-block[tool_output_is_signal] These emit the records of `codex exec --json`, which the real oneharness binary parses; its stdout is that protocol, one JSON record per transcript event, not human-facing output.
emit '{type:"thread.started", thread_id:"fake-codex-1"}'
emit '{type:"turn.started"}'
# shellcheck disable=SC2016 # the single-quoted programs are jq's; their $ names are jq variables, not shell expansions
emit --arg c "$ran" \
    '{type:"item.started", item:{id:"item_0", type:"command_execution", command:$c, aggregated_output:"", exit_code:null, status:"in_progress"}}'
# shellcheck disable=SC2016 # the single-quoted programs are jq's; their $ names are jq variables, not shell expansions
emit --arg c "$ran" --arg o "$output" --argjson x "$code" \
    '{type:"item.completed", item:{id:"item_0", type:"command_execution", command:$c, aggregated_output:$o, exit_code:$x, status:(if $x == 0 then "completed" else "failed" end)}}'
# shellcheck disable=SC2016 # the single-quoted programs are jq's; their $ names are jq variables, not shell expansions
emit --arg t "$reply" '{type:"item.completed", item:{id:"item_1", type:"agent_message", text:$t}}'
emit '{type:"turn.completed", usage:{input_tokens:11, cached_input_tokens:0, output_tokens:5}}'
# llmlint: ignore-end[tool_output_is_signal]
