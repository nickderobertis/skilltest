# Canonical command surface for skilltest (Rust core + pytest & vitest plugins).
#
# `just bootstrap` must work from a clean clone; `just check` is the full gate
# (format, lint, type check, unit + e2e) and fails on any issue. Each recipe
# fans out across the three stacks. Requires `cargo`, `uv`, and `pnpm` on PATH.

py := "plugins/pytest"
ts := "plugins/vitest"

# List available recipes.
default:
    @just --list

# Set up the project from a clean clone: fetch toolchains + dependencies.
bootstrap:
    cargo fetch
    cd {{py}} && uv sync
    cd {{ts}} && pnpm install

# Full quality gate: format check, lint, type check, unit tests, and e2e.
# Fails on any issue (no warnings-only mode). e2e is part of the gate.
check: format-check lint typecheck test test-e2e
    @echo "check: all gates passed"

# Build the Rust artifacts (the CLI + the fake provider the plugins drive).
build:
    cargo build

# Fast unit tests: the Rust library/bin unit suites.
test:
    cargo nextest run -E 'kind(lib) | kind(bin)'

# End-to-end suites across all three stacks, driving the built CLI as users do.
# The plugin suites shell out to the freshly built binaries, so build first.
test-e2e: build
    cargo nextest run -E 'kind(test)'
    cd {{py}} && SKILLTEST_BIN="$PWD/../../target/debug/skilltest" SKILLTEST_PROVIDER="$PWD/../../target/debug/skilltest-fake-provider" uv run pytest
    cd {{ts}} && SKILLTEST_BIN="$PWD/../../target/debug/skilltest" SKILLTEST_PROVIDER="$PWD/../../target/debug/skilltest-fake-provider" pnpm exec vitest run

# Lint the codebase; fail on findings.
lint:
    cargo clippy --all-targets -- -D warnings
    cd {{py}} && uv run ruff check .
    cd {{ts}} && pnpm exec biome check .

# Verify formatting without writing changes.
format-check:
    cargo fmt --check
    cd {{py}} && uv run ruff format --check .
    cd {{ts}} && pnpm exec biome format .

# Type check the typed stacks (Rust types are enforced by clippy/build).
typecheck:
    cd {{py}} && uv run ty check
    cd {{ts}} && pnpm exec tsc -p tsconfig.json

# Format the codebase in place.
format:
    cargo fmt
    cd {{py}} && uv run ruff format .
    cd {{ts}} && pnpm exec biome check --write .

# Security + license audit of the Rust dependency tree (not in the default gate;
# run before publishing binaries). Requires `cargo-deny`.
audit:
    cargo deny check

# Upgrade dependencies across all three stacks, then re-run the full gate.
upgrade:
    cargo update
    cd {{py}} && uv lock --upgrade && uv sync
    cd {{ts}} && pnpm update --latest
    @just check
