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

# Renderer for the terminal screenshots (`just screenshots`). NOT part of the
# gate or `just bootstrap`: screenshots are informational. CI's Visual-docs
# workflow installs the same pinned version; `just screenshots-tools` installs it
# locally on demand. screencomp (the classify/gallery/PR-comment tool) is
# installed separately — see https://github.com/nickderobertis/screencomp.
freeze-version := "0.2.2"

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
# e2e), plus the contract drift gate and the Rust coverage gate. Fails on any
# issue (no warnings-only mode). `test`/`test-e2e` run first as prerequisites
# (so the test suite is unambiguously part of the gate), then the static gates,
# then `coverage` enforces the line-coverage floor on the artifact's Rust core.
# Use `check-all` to force every project.
check: test test-e2e
    @bash scripts/gen-contract.sh --check
    {{nx}} affected -t format-check lint typecheck
    @just coverage
    @echo "check: all gates passed"

# Same gate, but across every project regardless of what changed.
check-all: coverage
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

# Coverage gate on the artifact's Rust core (skilltest-core + the skilltest CLI,
# including the binary e2e suite and the bundled fake provider). Measured with
# `cargo llvm-cov` over `cargo nextest` and FAILS below 95% line coverage — the
# create-repo default bar (see AGENTS.md "Stack and composition"). Always runs
# the whole Rust workspace (not nx-affected): the binary is the published
# artifact, so its coverage floor is proven on every gate run, not only when a
# Rust file changed. The non-default `fake-provider` feature is enabled so the
# e2e suite (which drives the bundled provider) is included in the measurement.
# Requires `cargo-llvm-cov` and `cargo-nextest`.
coverage:
    cargo llvm-cov nextest --workspace --features fake-provider --no-tests=pass --fail-under-lines 95

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

# --- Terminal screenshots (informational; never part of `check` or CI's gate) -
# Deterministic SVGs of the real CLI output, rendered by `freeze` from a vendored
# pinned font, gated/galleried/PR-commented by screencomp (see
# screenshots/AGENTS.md). Regenerating is out of the gate; CI's Visual-docs
# workflow owns the comparison, and the pre-push guard regenerates the baseline
# locally on drift.

# Install the pinned screenshot renderer (`freeze`) on demand. Needs Go.
screenshots-tools:
    @command -v go >/dev/null || { echo "go not found: needed to install freeze; see https://go.dev/dl" >&2; exit 1; }
    go install github.com/charmbracelet/freeze@v{{freeze-version}}
    @echo "installed freeze to $(go env GOPATH)/bin (ensure it is on PATH)"

# Capture the screenshots: drive the real binary against the bundled fake
# provider + the screenshots/fixture/ cases, render each scene to
# shots/current/<arch>/ + docs/screenshots/. Needs `freeze` on PATH.
screenshots:
    @bash scripts/screenshots.sh

# Regenerate the animated demo GIF (docs/screenshots/demo.gif — the README hero
# showing a typical `run` resolving case by case, then the report). Like the
# screenshots it drives the REAL release binary against the fake provider, then
# renders faithful frames with the vendored JetBrains Mono font (Pillow only — no
# ttyd/ffmpeg). It is informational, NOT hash-gated (a GIF isn't byte-reproducible),
# so regenerate on demand and commit the result. Needs Python 3 + Pillow.
screenshots-gif:
    @command -v python3 >/dev/null || { echo "python3 not found: needed to render the demo GIF" >&2; exit 1; }
    @python3 -c "import PIL" 2>/dev/null || { echo "Pillow not installed: pip install Pillow" >&2; exit 1; }
    cargo build --release --locked --features fake-provider -p skilltest-cli --bin skilltest --bin skilltest-fake-provider
    python3 scripts/demo-gif.py

# Refresh the committed baseline manifest from a fresh capture (after an intended
# output change). Commit shots/baseline/*.json + docs/screenshots/ alongside.
screenshots-bless: screenshots
    @command -v screencomp >/dev/null || { echo "screencomp not installed: https://github.com/nickderobertis/screencomp#install" >&2; exit 1; }
    screencomp manifest --input shots/current --output shots/baseline/$(uname -m | sed 's/amd64/x86_64/;s/aarch64/arm64/').json
    @echo "baseline refreshed; commit shots/baseline/ + docs/screenshots/"

# --- Live e2e against real harnesses (opt-in; never part of `just check`) ------
# These make real model calls (money, network, non-determinism), so they are
# kept out of the deterministic gate. They drive a real harness through
# `oneharness`; install it first with `just install-oneharness`. See docs/e2e.md.

# Install the prebuilt oneharness the live e2e drives (verifies the checksum).
# Keep in lockstep with `default_version` in scripts/install-oneharness.sh,
# which documents what each pinned version added.
install-oneharness version="v0.3.8":
    @bash scripts/install-oneharness.sh {{version}}

# Deep live suite against real oneharness + claude-code (needs CLAUDE_CODE_OAUTH_TOKEN
# + network). This is the suite CI's e2e-claude workflow runs.
test-live:
    cargo test -p skilltest-cli --test live -- --ignored

# Hermetic mock/spy suite against the REAL oneharness binary (no harness, no
# credentials, no model — a scripted claude shim executes the installed hook).
# Needs only `oneharness` on PATH (`just install-oneharness`); deterministic.
test-oneharness:
    cargo test -p skilltest-cli --test oneharness_integration -- --ignored

# Generic per-harness live smoke against a real harness (claude-code | opencode |
# goose | codex). Skips loudly when the harness / oneharness / secret is missing,
# or when the installed oneharness cannot yet carry the skill to that harness.
test-harness id:
    @bash scripts/e2e-harness.sh {{id}}

# Convenience: the claude-code live smoke via the generic per-harness path.
test-claude:
    @bash scripts/e2e-harness.sh claude-code

# Live check of the direct-API judge against the real Anthropic + OpenAI APIs
# (needs ANTHROPIC_API_KEY and/or OPENAI_API_KEY + network). Each vendor's test
# self-skips when its key is absent. This is the suite CI's e2e-judge-api runs.
test-judge-api:
    cargo test -p skilltest-cli --test live_api_judge -- --ignored --nocapture

# Install/refresh the optional llmlint toolchain. Idempotent.
setup-llmlint:
    ./scripts/setup-llmlint.sh

# Optional LLM-as-judge lint; non-deterministic and out of `check`.
lint-llm *paths:
    llmlint {{paths}}

# Deterministic llmlint config/ignore/version-bump validation.
lint-llm-validate *args:
    PATH="$HOME/.local/bin:$PATH" llmlint validate {{args}}

# llmlint scoped to changed files since the merge-base with main.
lint-llm-diff base="origin/main" *args:
    llmlint --diff --diff-base "{{base}}" {{args}}
