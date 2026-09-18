#!/usr/bin/env bash
# Journey test for `just lint-llm-config`: drives the recipe the way a
# contributor and CI do, over these trees —
#   - a config naming a plugin URL that does not resolve   -> refused, URL named
#   - a plugin removed upstream but still in the caller's
#     llmlint cache (a plain `llmlint validate` passes)     -> refused, URL named
#   - a directive naming a rule no loaded plugin defines   -> refused, rule named
#   - llmlint absent from PATH                              -> refused, fix named
#   - this repository's own configuration                   -> passes
# Needs llmlint, python3 (a local plugin server) and network (the real plugins
# are fetched), so it stays out of `just check`. Quiet on success apart from one
# summary line.
# llmlint: ignore-file[new_code_lands_in_a_project] repo-level llmlint glue beside setup-llmlint.sh: it tests the root llmlint.yml and the `just lint-llm-config` recipe, which belong to no Nx project (AGENTS.md: scripts/*.sh are orchestrator-independent glue).
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
just_bin="$(command -v just)"
work="$(mktemp -d)"
server_pid=""
cleanup() {
  [ -z "$server_pid" ] || kill "$server_pid" 2>/dev/null || true
  rm -rf "$work"
}
trap cleanup EXIT

plugins="https://raw.githubusercontent.com/nickderobertis/dero-skills/main/skills/bootstrap/create-repo/assets/llmlint"
fix_hint="fix scripts/lint-llm-config.sh (or the lint-llm-config recipe), then re-run \`just test-lint-llm-config\`"

fail() {
  printf 'test-lint-llm-config: FAIL: %s\n  next: %s\n' "$1" "$2" >&2
  [ -f "$work/out" ] && sed 's/^/  | /' "$work/out" >&2
  exit 1
}

# Run the recipe over DIR; leave its combined output in $work/out, echo the exit code.
run_recipe() {
  local status=0
  "$just_bin" --justfile "$root/justfile" lint-llm-config --cwd "$1" >"$work/out" 2>&1 || status=$?
  echo "$status"
}

expect_refusal() { # <status> <needle> <what>
  [ "$1" -ne 0 ] || fail "$3 passed validation" "$fix_hint"
  grep -qF "$2" "$work/out" || fail "$3 refused without naming \`$2\`" "$fix_hint"
}

missing_url="$plugins/does-not-exist.llmlint.yml"
mkdir "$work/missing-plugin"
printf 'plugins:\n  - "%s@1"\n' "$missing_url" >"$work/missing-plugin/llmlint.yml"
expect_refusal "$(run_recipe "$work/missing-plugin")" "$missing_url" "an unresolvable plugin URL"

mkdir -p "$work/www" "$work/stale-plugin" "$work/user-cache"
printf 'version: 1\n' >"$work/www/plugin.yml"
port="$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')"
python3 -m http.server "$port" --bind 127.0.0.1 --directory "$work/www" >/dev/null 2>&1 &
server_pid=$!
stale_url="http://127.0.0.1:$port/plugin.yml"
for _ in $(seq 50); do curl -sf -o /dev/null "$stale_url" && break; sleep 0.1; done
printf 'plugins:\n  - "%s@1"\n' "$stale_url" >"$work/stale-plugin/llmlint.yml"
LLMLINT_CACHE_DIR="$work/user-cache" llmlint validate --cwd "$work/stale-plugin" >"$work/out" 2>&1 \
  || fail "could not warm a cache from the local plugin server" "check python3 and llmlint are on PATH"
rm "$work/www/plugin.yml"
LLMLINT_CACHE_DIR="$work/user-cache" llmlint validate --cwd "$work/stale-plugin" >"$work/out" 2>&1 \
  || fail "llmlint no longer serves a removed plugin from cache, so this case proves nothing" \
    "drop the stale-cache case from this test"
status=0
LLMLINT_CACHE_DIR="$work/user-cache" "$just_bin" --justfile "$root/justfile" lint-llm-config \
  --cwd "$work/stale-plugin" >"$work/out" 2>&1 || status=$?
expect_refusal "$status" "$stale_url" "a plugin removed upstream but still cached"

mkdir "$work/unknown-rule"
printf 'plugins:\n  - "%s/languages/bash.llmlint.yml@1"\n' "$plugins" >"$work/unknown-rule/llmlint.yml"
# The directive is assembled at runtime so this script's own source never carries
# one (the repository-wide scan below would otherwise refuse it).
printf '#!/usr/bin/env bash\n# %s: ignore-file[no_such_rule] fixture: a rule no plugin defines\necho hi\n' 'llmlint' \
  >"$work/unknown-rule/example.sh"
expect_refusal "$(run_recipe "$work/unknown-rule")" 'unknown rule "no_such_rule"' "a directive naming an undefined rule"

status=0
HOME="$work" PATH="/usr/bin:/bin" "$just_bin" --justfile "$root/justfile" lint-llm-config \
  --cwd "$root" >"$work/out" 2>&1 || status=$?
expect_refusal "$status" "just setup-llmlint" "a run without llmlint installed"

status="$(run_recipe "$root")"
[ "$status" -eq 0 ] || fail "the repository's llmlint configuration was refused" \
  "fix the plugin URL or directive named above in llmlint.yml or the file it cites"

echo "test-lint-llm-config: ok"
