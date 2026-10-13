#!/usr/bin/env bash
# Scripted paid Goose boundary: oneharness owns prompt schema delivery/validation.
set -euo pipefail
prompt=""
args=("$@")
for ((i=0; i<${#args[@]}; i++)); do
    if [[ "${args[$i]}" == "-t" ]]; then prompt="${args[$((i+1))]}"; fi
done
if [[ -n "${SKILLTEST_BOUNDARY_LOG:-}" ]]; then
    jq -cn --arg prompt "$prompt" --arg role "${ROLE_MARKER:-goose}" \
        --arg model "${GOOSE_MODEL:-}" --arg mode "${GOOSE_MODE:-}" \
        --args '{prompt:$prompt,role:$role,model:$model,mode:$mode,argv:$ARGS.positional}' -- "${args[@]}" >> "$SKILLTEST_BOUNDARY_LOG"
fi
if [[ "$prompt" == *"single-line JSON object"* ]]; then
    if [[ "$prompt" == *"Score how well"* ]]; then
        printf '%s\n' '{"value":9,"reason":"goose numeric verdict"}'
    else
        printf '%s\n' '{"value":true,"reason":"goose boolean verdict"}'
    fi
else
    printf '%s\n' 'continue'
fi
