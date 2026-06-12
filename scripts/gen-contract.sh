#!/usr/bin/env bash
# Regenerate the CLI↔SDK contract artifacts, in dependency order:
#   Rust report types -> golden JSON Schemas (schemas/)
#                     -> generated SDK models (Pydantic for Python, type
#                        declarations for TypeScript).
#
# With --check, generate into a staging dir and verify the checked-in artifacts
# match instead of writing them — this is the drift gate `just check` runs, so
# a contract change that skips regeneration (or a hand-edit of generated code)
# fails CI with the exact diff.
#
# Adding a language: generate its models from "$stage/schemas" into the staged
# mirror of its SDK directory here, and append the outputs to `artifacts`.
set -euo pipefail
cd "$(dirname "$0")/.."

mode=write
if [[ "${1:-}" == "--check" ]]; then
  mode=check
fi

artifacts=(
  "schemas/report.schema.json"
  "schemas/validation.schema.json"
  "sdks/python/skilltest_sdk/_report.py"
  "sdks/python/skilltest_sdk/_validation.py"
  "sdks/typescript/src/generated/report.ts"
  "sdks/typescript/src/generated/validation.ts"
)

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/schemas" "$stage/sdks/python/skilltest_sdk" "$stage/sdks/typescript/src/generated"

# The ruff formatter datamodel-codegen invokes discovers its config from the
# *output* path, so pin one in the staging dir to keep --check and write
# byte-identical. Must match [tool.ruff] in sdks/python/pyproject.toml.
printf 'line-length = 100\ntarget-version = "py312"\n' > "$stage/ruff.toml"

cargo build -p skilltest-cli --quiet

./target/debug/skilltest schema report > "$stage/schemas/report.schema.json"
./target/debug/skilltest schema validation > "$stage/schemas/validation.schema.json"

# Python: pydantic models via datamodel-code-generator (pinned in uv.lock).
# Run from sdks/python so the ruff formatter it invokes picks up that project's
# config and the output matches the repo's formatting.
gen_python() {
  local name="$1"
  (cd sdks/python && PYTHONWARNINGS=ignore uv run --quiet datamodel-codegen \
    --input "$stage/schemas/$name.schema.json" \
    --input-file-type jsonschema \
    --output "$stage/sdks/python/skilltest_sdk/_$name.py" \
    --output-model-type pydantic_v2.BaseModel \
    --target-python-version 3.12 \
    --enum-field-as-literal all \
    --use-union-operator \
    --use-schema-description \
    --use-double-quotes \
    --disable-timestamp \
    --collapse-root-models \
    --use-title-as-name \
    --field-constraints \
    --formatters ruff-format)
}
gen_python report
gen_python validation

# TypeScript: type declarations via json-schema-to-typescript (pinned in
# pnpm-lock.yaml). Types only by design — the drift gate is what guarantees
# the shape, so the SDK does not re-validate it at runtime.
ts_banner='/* eslint-disable */
/**
 * Generated from the golden JSON Schemas in schemas/ by `just gen-contract`.
 * DO NOT MODIFY BY HAND — change the Rust report types and regenerate; the
 * contract drift gate fails while this file is stale.
 */'
gen_typescript() {
  local name="$1"
  pnpm --filter @skill-test/sdk --silent exec json2ts \
    -i "$stage/schemas/$name.schema.json" \
    -o "$stage/sdks/typescript/src/generated/$name.ts" \
    --additionalProperties false \
    --bannerComment "$ts_banner"
}
gen_typescript report
gen_typescript validation

status=0
for artifact in "${artifacts[@]}"; do
  if [[ "$mode" == "check" ]]; then
    if ! diff -u "$artifact" "$stage/$artifact" >/dev/null 2>&1; then
      echo "stale contract artifact: $artifact" >&2
      diff -u "$artifact" "$stage/$artifact" >&2 || true
      status=1
    fi
  else
    cp "$stage/$artifact" "$artifact"
  fi
done

if [[ "$status" -ne 0 ]]; then
  echo "contract artifacts are out of date with the Rust report types — run \`just gen-contract\` and commit the result" >&2
  exit "$status"
fi
