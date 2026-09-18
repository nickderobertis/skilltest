#!/usr/bin/env bash
# Validate the whole llmlint configuration, independent of any diff: every
# plugin URL is fetched from its origin (a throwaway LLMLINT_CACHE_DIR, so a
# cached copy can never mask a plugin renamed or removed upstream), the config
# structure is checked, and every `llmlint: ignore` directive in the tree must
# name a rule some loaded plugin defines. Deterministic apart from the plugin
# fetches; no model call. Extra args pass through to `llmlint validate` (e.g.
# `--diff-base origin/main` adds the version-bump check against that base,
# `--cwd DIR` validates another tree).
#
# Usage: scripts/lint-llm-config.sh [llmlint validate args...]
set -euo pipefail

if ! command -v llmlint >/dev/null 2>&1; then
  echo "lint-llm-config: llmlint not found on PATH; run \`just setup-llmlint\`" >&2
  exit 127
fi

cache_dir="$(mktemp -d)"
trap 'rm -rf "$cache_dir"' EXIT

LLMLINT_CACHE_DIR="$cache_dir" llmlint validate "$@"
