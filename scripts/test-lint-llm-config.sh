#!/usr/bin/env bash
# Journey test for `just lint-llm-config`: drives the recipe the way a
# contributor and CI do, over three trees —
#   1. a config naming a plugin URL that does not resolve  -> refused, URL named
#   2. a directive naming a rule no loaded plugin defines  -> refused, rule named
#   3. this repository's own configuration                 -> passes
# Needs llmlint and network (the plugins are fetched), so it stays out of
# `just check`. Quiet on success apart from one summary line.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

plugins="https://raw.githubusercontent.com/nickderobertis/dero-skills/main/skills/bootstrap/create-repo/assets/llmlint"

fail() {
  printf 'test-lint-llm-config: FAIL: %s\n' "$1" >&2
  [ -f "$work/out" ] && sed 's/^/  | /' "$work/out" >&2
  exit 1
}

# Run the recipe over DIR; leave its combined output in $work/out, echo the exit code.
run_recipe() {
  local status=0
  just --justfile "$root/justfile" lint-llm-config --cwd "$1" >"$work/out" 2>&1 || status=$?
  echo "$status"
}

# 1. A plugin URL that 404s (the shape of an upstream rename) is refused by name.
missing_url="$plugins/does-not-exist.llmlint.yml"
mkdir "$work/missing-plugin"
printf 'plugins:\n  - "%s@1"\n' "$missing_url" >"$work/missing-plugin/llmlint.yml"
status="$(run_recipe "$work/missing-plugin")"
[ "$status" -ne 0 ] || fail "config with an unresolvable plugin URL passed"
grep -qF "$missing_url" "$work/out" || fail "unresolvable plugin refused without naming its URL"

# 2. A directive naming a rule no loaded plugin defines is refused by name.
mkdir "$work/unknown-rule"
printf 'plugins:\n  - "%s/languages/bash.llmlint.yml@1"\n' "$plugins" >"$work/unknown-rule/llmlint.yml"
# The directive is assembled at runtime so this script's own source never carries
# one (the repository-wide scan in step 3 would otherwise refuse it).
printf '#!/usr/bin/env bash\n# %s: ignore-file[no_such_rule] fixture: a rule no plugin defines\necho hi\n' 'llmlint' \
  >"$work/unknown-rule/example.sh"
status="$(run_recipe "$work/unknown-rule")"
[ "$status" -ne 0 ] || fail "directive naming an undefined rule passed"
grep -qF 'unknown rule "no_such_rule"' "$work/out" || fail "undefined rule refused without naming it"

# 3. This repository's own configuration passes.
status="$(run_recipe "$root")"
[ "$status" -eq 0 ] || fail "the repository's llmlint configuration was refused"

echo "test-lint-llm-config: ok"
