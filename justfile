# Canonical command surface for skilltest (Rust core + per-language SDKs +
# per-framework test packages).
#
# `just` is a thin wrapper over **nx**: each recipe drives the per-project targets
# defined in the `project.json` files, and the dependency graph (core <- cli <-
# {python sdk, ts sdk} <- {pytest, vitest}) lets nx build prerequisites and skip
# unaffected work.
#
# `just check` runs the gate over only the **affected** projects (vs the nx base,
# `main`) plus the contract drift gate, which is workspace-level and always runs;
# `just check-all` forces every project. `just bootstrap` must work from a clean
# clone. Requires `cargo` (+ `cargo-nextest`), `uv`, and `pnpm`/`node`.

nx := "pnpm exec nx"

# List available recipes.
default:
    @just --list

# Set up the project from a clean clone: install nx + per-stack dependencies.
# The root `pnpm install` covers nx and the whole TS workspace (both packages).
bootstrap:
    pnpm install
    cargo fetch
    cd sdks/python && uv sync
    cd plugins/pytest && uv sync

# Full quality gate over the affected projects (format, lint, type check, unit +
# e2e) plus the contract drift gate. Fails on any issue (no warnings-only mode).
# Use `check-all` to force every project.
check:
    @bash scripts/gen-contract.sh --check
    {{nx}} affected -t format-check lint typecheck test test-e2e
    @echo "check: all gates passed"

# Same gate, but across every project regardless of what changed.
check-all:
    @bash scripts/gen-contract.sh --check
    {{nx}} run-many -t format-check lint typecheck test test-e2e
    @echo "check-all: all gates passed"

# Build the artifacts for affected projects (Rust CLI + fake provider, TS dist).
build:
    {{nx}} affected -t build

# Regenerate the contract artifacts: the golden JSON Schemas in schemas/ from
# the Rust report types, then every SDK's generated models from the schemas.
# Run this whenever the report types change; `check` fails while it is stale.
gen-contract:
    @bash scripts/gen-contract.sh

# Drift gate: verify the checked-in contract artifacts match what the Rust
# types generate (part of `just check`; workspace-level, not per-project).
contract-check:
    @bash scripts/gen-contract.sh --check

# Fast unit tests (Rust library/bin suites) for affected projects.
test:
    {{nx}} affected -t test

# End-to-end suites for affected projects, driving the built CLI as users do.
# nx builds prerequisites first via the project graph (SDKs depend on
# skilltest-cli; framework packages depend on their SDK).
test-e2e:
    {{nx}} affected -t test-e2e

# Lint affected projects; fail on findings.
lint:
    {{nx}} affected -t lint

# Verify formatting of affected projects without writing changes.
format-check:
    {{nx}} affected -t format-check

# Type check affected projects (Rust types are enforced by clippy/build).
typecheck:
    {{nx}} affected -t typecheck

# Format every project in place.
format:
    {{nx}} run-many -t format

# Show the project graph (opens the interactive nx graph).
graph:
    {{nx}} graph

# Security + license audit of the Rust dependency tree (not in the default gate;
# run before publishing binaries). Requires `cargo-deny`.
audit:
    cargo deny check

# Upgrade dependencies across all stacks (nx + the three toolchains), then re-run
# the full gate across every project.
upgrade:
    pnpm -r update --latest
    cargo update
    cd sdks/python && uv lock --upgrade && uv sync
    cd plugins/pytest && uv lock --upgrade && uv sync
    @just check-all

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
