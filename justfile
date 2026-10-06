# Canonical command surface for skilltest (Rust core + per-language SDKs +
# per-framework test packages).
#
# `just` is a thin wrapper over **nx**: each recipe drives the per-project targets
# defined in the `project.json` files, and the dependency graph (core <- cli <-
# {python sdk, ts sdk} <- {pytest, vitest}) lets nx build prerequisites and skip
# unaffected work.
#
# `just check` is the one gate recipe, and its tier is a flag: `just check` runs
# the AFFECTED tier (the projects a change can reach, against an explicitly
# derived base) and `just check all` the BROADER tier (every project). Both add
# the workspace-level gates that always run — contract drift, release targets,
# module boundaries, workflow routing, Rust coverage. `just bootstrap` must work
# from a clean clone. Requires `cargo` (+ `cargo-nextest`, `cargo-llvm-cov`),
# `uv`, `pnpm`/`node`, `jq`, and `python3` 3.11+.

nx := "pnpm exec nx"

# The affected tier's selection: `nx affected` against the commit
# scripts/nx-base.sh derives — NX_BASE when it is a plain ref or SHA that
# resolves (CI exports it with nx-set-shas), else the merge base with
# origin/main; anything else fails closed. Never Nx's implicit default base.
affected := 'base="$(bash scripts/nx-base.sh)" && ' + nx + ' affected --base="$base"'

# The one selection rule that keeps the live tier out of both gate tiers: every
# project tagged `type:live` (its suite makes real model/API calls and runs only
# from its own workflow). A live project added with that tag stays out with no
# edit here. See AGENTS.md "Project graph".
live := "tag:type:live"

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
# The root `pnpm install` covers nx and the whole TS workspace (both packages);
# the root `uv sync` covers the uv workspace (both Python packages, one lock).
bootstrap:
    pnpm install
    cargo fetch
    uv sync

# The quality gate; fails on any issue (no warnings-only mode). `tier` selects
# which projects run the per-project targets (format, lint, type check, unit,
# the CLI's binary e2e project, the SDK/plugin e2e):
#   affected (default) — the projects this change can reach; what a PR runs.
#   all                — every project; the broader-tier sweep CI runs at
#                        merge-to-main, ahead of the release.
# Live projects are excluded from both. Their code still lints here (compile-
# but-skip: the live tier must not rot), but no live suite runs from this
# recipe. Then the workspace-level gates, which run on every tier because they
# span every stack, and the Rust coverage floor. just resolves the tier itself,
# so a mistyped one aborts before anything runs.
check tier="affected":
    @{{ if tier == "affected" { "true" } else if tier == "all" { "true" } else { error("unknown tier '" + tier + "' — use 'affected' (the default) or 'all'") } }}
    @just contract-check
    @just release-targets-check
    @just boundaries-check
    @just workflows-check
    {{ if tier == "all" { "pnpm exec nx run-many" } else { affected } }} -t format-check lint typecheck test test-e2e --exclude={{live}}
    {{nx}} run-many -t lint format-check --projects={{live}}
    @just coverage
    @echo "check ({{tier}}): all gates passed"

# Build the artifacts for affected projects (Rust CLI + fake provider, TS dist).
build:
    {{affected}} -t build --exclude={{live}}

# Regenerate the contract artifacts: the golden JSON Schemas in schemas/ from
# the Rust report types, then every SDK's generated models from the schemas.
# Run this whenever the report types change; `check` fails while it is stale.
gen-contract:
    @{{nx}} run skilltest-contract:gen-contract --output-style=stream-without-prefixes

# Drift gate: verify the checked-in contract artifacts match what the Rust
# types generate — the `skilltest-contract` project's target, which every tier
# of `just check` runs (uncached) because the contract spans every stack.
contract-check:
    @{{nx}} run skilltest-contract:contract-check --output-style=stream-without-prefixes

# Release-target gate (part of `just check`; workspace-level, not per-project,
# so it runs even when only release-targets.toml or a workflow changed): the
# declaration against what publish.yml publishes, that gate's own drift tests,
# and the release probe's offline outcome tests (curl doubled; no network).
release-targets-check:
    @bash scripts/check-release-targets.sh >/dev/null
    @bash scripts/check-release-targets-test.sh >/dev/null
    @bash scripts/check-release-probe.sh >/dev/null

# Live drift alarm for the release probe: drives it against the real crates.io,
# PyPI and npm for every declared target. Network, so never part of `check`.
release-probe-live:
    @bash scripts/release-probe-live.sh

# Module-boundary gate (part of every `just check` tier; workspace-level): every
# project's `type:*`/`lang:*` tags and every graph edge against the allowed
# edges, read from the graph nx computes — plus the checker's own red/green test.
boundaries-check:
    @tmp="$(mktemp -d)" && trap 'rm -rf "$tmp"' EXIT && {{nx}} graph --file="$tmp/graph.json" >/dev/null && python3 scripts/check-project-boundaries.py "$tmp/graph.json" >/dev/null
    @python3 scripts/check-project-boundaries-test.py >/dev/null

# Workflow-routing gate (part of every `just check` tier; workspace-level): which
# jobs each event runs across .github/workflows — the PR tier, the merge-to-main
# sweep + live suites gating the release, the `chore(release):` commit running
# nothing, the tag workflows — plus its own red/green test. Needs `uv`.
workflows-check:
    @uv run --quiet --script scripts/check-workflow-routing.py >/dev/null

# The `test` target of affected projects: the Rust unit suites and the CLI's
# binary e2e project (`skilltest-cli-e2e`, which builds the CLI first).
test:
    {{affected}} -t test --exclude={{live}}

# The SDK/framework e2e suites of affected projects, driving the built CLI as
# users do. nx builds prerequisites first via the project graph (SDKs depend on
# skilltest-cli; framework packages depend on their SDK).
test-e2e:
    {{affected}} -t test-e2e --exclude={{live}}

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
    {{affected}} -t lint --exclude={{live}}

# Verify formatting of affected projects without writing changes.
format-check:
    {{affected}} -t format-check --exclude={{live}}

# Type check affected projects (Rust types are enforced by clippy/build).
typecheck:
    {{affected}} -t typecheck --exclude={{live}}

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
# the full gate as the broader tier: an upgrade can reach every project.
upgrade:
    pnpm -r update --latest
    cargo update
    uv lock --upgrade && uv sync
    @just check all

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
# kept out of the deterministic gate: each is an nx project tagged `type:live`
# (the selection rule `check` excludes) and runs from its own workflow, which CI
# calls at merge-to-main ahead of the release. They drive a real harness through
# `oneharness`; install it first with `just install-oneharness`. See docs/e2e.md.

# Install the prebuilt oneharness the live e2e drives (verifies the checksum).
# The version is authored in scripts/install-oneharness.sh; this default and
# both SDKs' bounds restate it, and the gate's
# `oneharness_pin_is_lockstep_across_installer_recipe_and_both_sdks` reconciles
# all four.
install-oneharness version="v0.16.0":
    @bash scripts/install-oneharness.sh {{version}}

# Deep live suite against real oneharness + claude-code (needs CLAUDE_CODE_OAUTH_TOKEN
# + network). This is the suite CI's e2e-claude workflow runs; nx builds the CLI
# first.
test-live:
    {{nx}} run skilltest-live-claude:live --output-style=stream-without-prefixes

# Hermetic mock/spy suite against the REAL oneharness binary (no harness, no
# credentials, no model — a scripted claude shim executes the installed hook).
# Needs only `oneharness` on PATH (`just install-oneharness`); deterministic.
test-oneharness:
    cargo test -p skilltest-cli --test oneharness_integration -- --ignored

# Generic per-harness live smoke against a real harness (claude-code | opencode |
# goose | codex). Skips loudly when the harness / oneharness / secret is missing,
# or when the installed oneharness cannot yet carry the skill to that harness.
test-harness id:
    @{{nx}} run skilltest-live-harness:live --harness={{id}} --output-style=stream-without-prefixes

# Convenience: the claude-code live smoke via the generic per-harness path.
test-claude:
    @just test-harness claude-code

# Live check of the direct-API judge against the real Anthropic + OpenAI APIs
# (needs ANTHROPIC_API_KEY and/or OPENAI_API_KEY + network). Each vendor's test
# self-skips when its key is absent. This is the suite CI's e2e-judge-api runs.
test-judge-api:
    {{nx}} run skilltest-live-judge-api:live --output-style=stream-without-prefixes

# Install/refresh the optional llmlint toolchain. Idempotent.
setup-llmlint:
    ./scripts/setup-llmlint.sh

# Optional LLM-as-judge lint; non-deterministic and out of `check`.
# Harness/model routing — `run_mode` included — comes from the repo-root
# `oneharness.toml`, which the oneharness line the SDKs bundle parses in full.
lint-llm *paths:
    llmlint {{paths}}

# Deterministic llmlint config/ignore/version-bump validation.
lint-llm-validate *args:
    PATH="$HOME/.local/bin:$PATH" llmlint validate {{args}}

# Whole-config llmlint validation, independent of any diff: every plugin URL
# fetched fresh (never from cache), every ignore directive's rule known. CI's
# llmlint job runs this before judging anything. Extra args go to `llmlint validate`.
lint-llm-config *args:
    @PATH="$HOME/.local/bin:$PATH" bash scripts/lint-llm-config.sh {{args}}

# Journey test for `lint-llm-config` (needs llmlint + network; not in `check`).
test-lint-llm-config:
    @PATH="$HOME/.local/bin:$PATH" bash scripts/test-lint-llm-config.sh

# llmlint scoped to changed files since the merge-base with main.
lint-llm-diff base="origin/main" *args:
    llmlint --diff --diff-base "{{base}}" {{args}}
