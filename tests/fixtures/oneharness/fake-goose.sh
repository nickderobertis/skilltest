#!/usr/bin/env bash
# llmlint: ignore-file[new_code_lands_in_a_project] The dispatch requires a shared non-Claude paid-boundary fixture. nx.json sharedGlobals explicitly hashes tests/fixtures for the existing CLI/SDK integration projects; this script has no independent runtime or project target.
# Scripted paid Goose boundary: oneharness owns prompt schema delivery/validation.
set -euo pipefail
command -v jq >/dev/null 2>&1 || {
    echo "fake-goose: jq is unavailable; install jq and rerun the oneharness integration test" >&2
    exit 2
}
prompt=""
args=("$@")
for ((i=0; i<${#args[@]}; i++)); do
    if [[ "${args[$i]}" == "-t" ]]; then prompt="${args[$((i+1))]}"; fi
done
if [[ -n "${SKILLTEST_BOUNDARY_LOG:-}" ]]; then
    if ! jq -cn --arg prompt "$prompt" --arg role "${ROLE_MARKER:-goose}" \
        --arg model "${GOOSE_MODEL:-}" --arg mode "${GOOSE_MODE:-}" \
        --args '{prompt:$prompt,role:$role,model:$model,mode:$mode,argv:$ARGS.positional}' -- "${args[@]}" >> "$SKILLTEST_BOUNDARY_LOG"; then
        echo "fake-goose: could not append boundary observations; fix SKILLTEST_BOUNDARY_LOG path/permissions and rerun the integration test" >&2
        exit 2
    fi
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
