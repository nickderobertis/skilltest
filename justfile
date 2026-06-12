# Canonical command surface for skilltest (Rust core + per-language SDKs +
# per-framework test packages).
#
# `just bootstrap` must work from a clean clone; `just check` is the full gate
# (format, lint, type check, unit + e2e) and fails on any issue. Each recipe
# fans out across the three stacks. Requires `cargo`, `uv`, and `pnpm` on PATH.
# The TypeScript packages live in a pnpm workspace rooted here.

py-sdk := "sdks/python"
py-pytest := "plugins/pytest"

# List available recipes.
default:
    @just --list

# Set up the project from a clean clone: fetch toolchains + dependencies.
bootstrap:
    cargo fetch
    cd {{py-sdk}} && uv sync
    cd {{py-pytest}} && uv sync
    pnpm install

# Full quality gate: format check, lint, type check, unit tests, and e2e.
# Fails on any issue (no warnings-only mode). e2e is part of the gate.
check: format-check lint typecheck test test-e2e
    @echo "check: all gates passed"

# Build the Rust artifacts (the CLI + the fake provider the SDKs drive).
build:
    cargo build

# Regenerate the golden JSON Schemas in schemas/ from the Rust report types.
# The Rust e2e suite fails when these drift; the SDK contract tests compare
# their models against them. Run this whenever the report types change.
gen-schemas: build
    ./target/debug/skilltest schema report > schemas/report.schema.json
    ./target/debug/skilltest schema validation > schemas/validation.schema.json

# Fast unit tests: the Rust library/bin unit suites.
test:
    cargo nextest run -E 'kind(lib) | kind(bin)'

# End-to-end suites across all stacks, driving the built CLI as users do. The
# SDK/framework suites shell out to the freshly built binaries, so build first;
# the vitest plugin re-exports the built @skilltest/sdk, so build that too.
test-e2e: build
    cargo nextest run -E 'kind(test)'
    cd {{py-sdk}} && SKILLTEST_BIN="$PWD/../../target/debug/skilltest" SKILLTEST_PROVIDER="$PWD/../../target/debug/skilltest-fake-provider" uv run pytest
    cd {{py-pytest}} && SKILLTEST_BIN="$PWD/../../target/debug/skilltest" SKILLTEST_PROVIDER="$PWD/../../target/debug/skilltest-fake-provider" uv run pytest
    pnpm -r run build
    SKILLTEST_BIN="$PWD/target/debug/skilltest" SKILLTEST_PROVIDER="$PWD/target/debug/skilltest-fake-provider" pnpm -r --workspace-concurrency=1 run test

# Lint the codebase; fail on findings.
lint:
    cargo clippy --all-targets -- -D warnings
    cd {{py-sdk}} && uv run ruff check .
    cd {{py-pytest}} && uv run ruff check .
    pnpm exec biome check .

# Verify formatting without writing changes.
format-check:
    cargo fmt --check
    cd {{py-sdk}} && uv run ruff format --check .
    cd {{py-pytest}} && uv run ruff format --check .
    pnpm exec biome format .

# Type check the typed stacks (Rust types are enforced by clippy/build). The
# vitest plugin's tsc resolves @skilltest/sdk from its built dist, so build it.
typecheck:
    cd {{py-sdk}} && uv run ty check
    cd {{py-pytest}} && uv run ty check
    pnpm --filter @skilltest/sdk run build
    pnpm -r --workspace-concurrency=1 run typecheck

# Format the codebase in place.
format:
    cargo fmt
    cd {{py-sdk}} && uv run ruff format .
    cd {{py-pytest}} && uv run ruff format .
    pnpm exec biome check --write .

# Security + license audit of the Rust dependency tree (not in the default gate;
# run before publishing binaries). Requires `cargo-deny`.
audit:
    cargo deny check

# Upgrade dependencies across all three stacks, then re-run the full gate.
upgrade:
    cargo update
    cd {{py-sdk}} && uv lock --upgrade && uv sync
    cd {{py-pytest}} && uv lock --upgrade && uv sync
    pnpm -r update --latest
    @just check

# --- Live e2e against real harnesses (opt-in; never part of `just check`) ------
# These make real model calls (money, network, non-determinism), so they are
# kept out of the deterministic gate. They drive a real harness through
# `oneharness`; install it first with `just install-oneharness`. See docs/e2e.md.

# Install the prebuilt oneharness the live e2e drives (verifies the checksum).
# v0.2.1 first drove codex/goose (not just claude-code); v0.2.37 extracts
# OpenCode's final text from its JSONL and ships qwen's yolo-warning suppression.
install-oneharness version="v0.2.37":
    @bash scripts/install-oneharness.sh {{version}}

# Deep live suite against real oneharness + claude-code (needs CLAUDE_CODE_OAUTH_TOKEN
# + network). This is the suite CI's e2e-claude workflow runs.
test-live:
    cargo test -p skilltest-cli --test live -- --ignored

# Generic per-harness live smoke against a real harness (claude-code | opencode |
# goose | codex). Skips loudly when the harness / oneharness / secret is missing,
# or when the installed oneharness cannot yet carry the skill to that harness.
test-harness id:
    @bash scripts/e2e-harness.sh {{id}}

# Convenience: the claude-code live smoke via the generic per-harness path.
test-claude:
    @bash scripts/e2e-harness.sh claude-code
