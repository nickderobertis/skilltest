# AGENTS.md <!-- llmlint: ignore[instruction_layer_localized] Review-ownership routing (CODEOWNERS) is repository policy, not content this instruction file can carry. -->

Durable instructions for humans and agents working in this repo. Write for a
future maintainer, not as a session log. Put deterministic steps in scripts and
keep this file for constraints, tradeoffs, and judgment.

> `CLAUDE.md` is a symlink to this file (`ln -s AGENTS.md CLAUDE.md`) so the two
> never drift. Edit `AGENTS.md` only.

## What this repo is

`skilltest` is a framework for testing AI **skills** (a `SKILL.md` plus its
assets). The artifact is a **Rust CLI** (`skilltest`), one thin **SDK per
language** that wraps the CLI and nothing else (`skilltest-sdk` for Python,
`@skill-test/sdk` for TypeScript), and one **package per test framework** built
on its language's SDK — `pytest` and `vitest` to start, with more frameworks
and languages to come. It runs a skill on one or more harness/model
**platforms** via [`oneharness`](https://github.com/nickderobertis/oneharness),
optionally driving a simulated user across multiple turns, then scores the
transcript with built-in **natural-language evals** (boolean and numeric). It
also validates skill definitions.

Consumers: skill authors who want a regression suite for a skill, and CI that
must prove a skill still behaves.

## Two standing goals on every task

The user drives product features and their request is the priority — but carry
two goals into *every* task. When either is the lowest-error path to what the
user asked, fold it into the same task without asking first; surface the rest as
follow-ups (see "After the main task").

1. **Engineer the context for next time.** Make the next agent (and you) see
   more for less: realistic end-to-end tests that exercise what users actually
   see — especially when they report a bug existing tests missed (the e2e suites
   drive the built CLI as a subprocess, see "Tests are context engineering") —
   scripts and skills that automate repetitive steps and shrink their output to
   signal, and terse `AGENTS.md` notes capturing what the code doesn't make
   obvious.
2. **Engineer the codebase and environment.** Be the engineer the user isn't:
   prioritize the technical initiatives that keep the codebase clean,
   maintainable, and repeatable, and keep environment setup automated and
   consistent (`just bootstrap` from a clean clone). Strict quality gates plus
   local/CI parity (the same `just check`, the same pinned toolchains) make
   results repeatable — not "works on my machine." A clean base and a
   reproducible environment are usually how the user's feature ships with a low
   error rate.

## Stack and composition

Composed from the `create-repo` skill's references (dero-skills v1.47.3) rather
than one template — `compose_repo_plan.py --shape cli --language rust --language
python --language typescript --language bash --releasing`, with `shapes/library.md`
added. What was pulled in, and why:

- **Always: `base.md`, `project-graph.md`, `ci.md`, `llmlint.md`.** `base.md` sets
  the invariants below. `project-graph.md` makes the repo an Nx project graph
  split by tier and cost ("Project graph" below). `ci.md` gives clean checkout →
  `just bootstrap` → `just check` on a Linux/macOS matrix, the two staged gate
  tiers, the live tier in its own credential-gated workflows, the install-path
  smoke (`bundle-smoke.yml`) and the notignored suppressions comment. `llmlint.md`
  gives the LLM-judge tier: `llmlint.yml` composing the per-reference fragments,
  the fallback `oneharness.toml`, and the blocking `llmlint` PR job.
- **Product shape — `shapes/cli.md` + `shapes/library.md`.** The shipped artifact
  is a compiled CLI (`skilltest`), tested as the *built* binary in the gate;
  `skilltest-core` is a reusable library with a stable, documented API and the
  source of truth for the JSON contract.
- **Languages — `languages/rust.md` + `intersections/rust-cli.md`** set the Rust
  toolchain (pinned in `rust-toolchain.toml`) and gates: `rustfmt`, `clippy -D
  warnings`, `cargo nextest` per project, the binary e2e tier as its own crate,
  `cargo llvm-cov` coverage, and `cargo deny` (`just audit`). **`languages/
  python.md`** (uv/ruff/ty/pytest) and **`languages/typescript.md`**
  (biome/tsc/vitest) cover the thin SDKs and framework packages, each on its own
  native toolchain. **`languages/bash.md`** covers the `scripts/*.sh` build,
  release and gate glue and the live harness lane (`set -euo pipefail`,
  shellcheck-clean, quiet on success).
- **`releasing.md`** — Conventional Commits drive semantic-release; tags drive
  the decoupled build/publish workflows ("Commits, releases, and merging").
- **Each ecosystem keeps one workspace root** — `Cargo.toml`, `pnpm-workspace.yaml`
  and the root `pyproject.toml`'s `[tool.uv.workspace]` — so it resolves into one
  lockfile, never one per package.
- **Excluded, and why.** `shapes/nextjs.md`, `shapes/web-app.md`,
  `shapes/react.md` — there is no web UI. `shapes/skills-repo.md` — skilltest
  *tests* skills but is not itself a skills repo. `shapes/asdf-plugin.md` — the
  CLI installs through `scripts/install.sh`, cargo and the bundling SDKs, not
  asdf. `languages/terraform.md` — no infrastructure. `intersections/
  python-cli.md` — the Python package is a thin SDK/library wrapping the Rust
  CLI, with no console entry point of its own.
- **TypeScript package manager: pnpm.** `typescript.md` defaults to bun and
  allows pnpm/npm only where a constraint rules bun out. The pnpm workspace
  predates that default, and no constraint ruling bun out is known, so moving
  to bun is open. A migration has to re-prove what the release path relies on:
  `pnpm publish` rewriting `workspace:*`, and `pnpm/action-setup` in CI.

### Coverage and e2e (the gate's depth)

- **Coverage — enforced, 95% lines.** `just coverage`, in every `just check`
  tier, runs `cargo llvm-cov nextest --workspace --features fake-provider
  --fail-under-lines 95` and fails the gate below 95% line coverage of the Rust
  core's sources (`skilltest-core` + the `skilltest` CLI, the fake provider
  included); the measured files are the two crates' `src/`, and the test-only
  crates contribute no lines. It runs over the **whole Rust workspace** on every
  gate, not only when nx calls a Rust
  project affected: the binary is the published artifact, so its floor is proven
  on every run. The Python/TS SDKs are proven by their `test-e2e` targets and the
  bundled-binary install smoke; no coverage bar is enforced on them.
- **E2E — real, in the gate.** The CLI's binary e2e tier is its own project
  (`crates/skilltest-cli-e2e`), driving the **built** CLI as a subprocess against
  `skilltest-fake-provider` (only the model is faked); each SDK and framework
  package has a `test-e2e` target doing the same through its own API. Each suite
  covers a happy path **and** ≥1 failure/recovery path. The **live** tier that
  needs real harnesses/APIs is out of the gate: its projects (`live/`) are
  compiled and linted by every gate (`#[ignore]`d at runtime) and run only from
  their own workflows. See `docs/e2e.md`.

## Layout

| Path | What |
| --- | --- |
| `crates/skilltest-core` | Library: config, skill + case models, the provider protocol and backends, evals, mocking, runner, report; the source of truth for the JSON contract. |
| `crates/skilltest-cli` | The `skilltest` binary, plus the feature-gated `skilltest-fake-provider` reference provider. |
| `crates/skilltest-cli-e2e` | The CLI's binary e2e tier (`publish = false`). |
| `schemas/` | The generated CLI↔SDK contract (JSON Schemas) — project `skilltest-contract`. |
| `sdks/python`, `sdks/typescript` | `skilltest-sdk` / `@skill-test/sdk`: one thin CLI wrapper per language, bundling the CLI. |
| `sdks/typescript/platforms/cli-*` | The four `@skill-test/cli-*` npm packages that carry the prebuilt binary. |
| `plugins/pytest`, `plugins/vitest` | `skilltest-pytest` / `@skill-test/vitest`: one package per test framework, on its language's SDK. |
| `live/{claude,judge-api,harness}` | The live suites: the deep claude-code suite, the direct-API judge, the per-harness smoke. |
| `pyproject.toml`, `uv.lock` | The uv workspace root over `sdks/python` + `plugins/pytest`; publishes nothing. |
| `tests/fixtures` | Sample skills and cases the e2e suites share (`tests/AGENTS.md`). |
| `docs/` | The provider protocol, config/case schema, development and live-e2e references. |
| `scripts/` | Repo-level glue, orchestrator-independent: contract generation, the release-target, module-boundary and workflow-routing gates, the affected-tier base (`nx-base.sh`), install/bundle/smoke/version scripts, screenshots. |
| `release-targets.toml` | What a release publishes, for onevcs; ids and short names are a cross-repository contract ("Commits, releases, and merging"). |
| `screenshots/`, `screencomp.toml`, `shots/baseline/`, `docs/screenshots/` | Terminal screenshots, informational, never a gate (`screenshots/AGENTS.md`). |
| `gh-secrets.json` | Declarative secret manifest, synced from Bitwarden via `gh-secrets manifest sync`. |
| `.github/workflows/` | `ci.yml` (both gate tiers, the live calls, the release), `e2e-*.yml` (one live suite each), `semantic-release.yml`, `release.yml`/`publish.yml` (tag-triggered), `bundle-smoke.yml`, `visual-docs.yml`, `pr-title.yml`, `notignored.yml`. |
| `nx.json`, `<project>/project.json` | The Nx workspace: named inputs, target defaults, and each project's targets, tags and edges. |
| `rust-toolchain.toml` | The one Rust toolchain pin (channel, components, release targets); every workflow installs from it, and `just workflows-check` holds its targets to the build matrices. |

## Project graph

Nx owns running targets; each ecosystem's workspace owns dependency resolution.
The graph, with each project's `type:`/`lang:` tags:

- `skilltest-core` (`type:lib`, rust) ← `skilltest-cli` (`type:app`, rust).
- `skilltest-contract` (`type:contract`, json) — `schemas/` plus its inputs
  `scripts/gen-contract.sh` and `tests/fixtures/contract/`.
- `skilltest-cli-e2e` (`type:e2e`, rust) → cli, core, contract.
- `skilltest-sdk` (`type:sdk`, python) and `@skill-test/sdk` (`type:sdk`,
  typescript) → cli, contract (the TS SDK also → its four `type:carrier`
  packages); `skilltest-pytest` / `@skill-test/vitest` (`type:plugin`) → their SDK.
- `skilltest-live-claude`, `skilltest-live-judge-api`, `skilltest-live-harness`
  (`type:live`) → cli and/or core.

**Module boundaries.** Every project carries exactly one `type:` and one `lang:`
tag, and `just boundaries-check` (every `just check` tier) holds each graph edge
to the `ALLOWED` table in `scripts/check-project-boundaries.py` — the one source
for which type may depend on which. Its intent: nothing may depend on an `e2e`
or `live` project, so an expensive suite stays behind an edge no library can
draw back, and a `contract` depends only on contracts, never on its consumers.

**The live selection rule:** every project tagged `type:live` is excluded from
both tiers of `just check` (`--exclude=tag:type:live`); a live project added with
that tag stays out with no recipe edit. The gate still lints those projects
(compile-but-skip), but no live suite runs from it.

**Cache keys** (`nx.json`): `lint`, `typecheck`, `test` and `test-e2e` hash their
own inputs **and** their dependencies' (`^default`), so a change to a Rust crate
reruns the SDK and plugin targets downstream rather than replaying them.
`schemas/` is the contract project's input, not a shared global, so a schema
edit selects the contract and its dependents, not every project.

## Command surface

Use the `just` recipes; do not hand-roll equivalent commands. They delegate to
nx, which builds prerequisites in graph order.

- `just bootstrap` — set up from a clean clone (each stack from its workspace root).
- `just check [tier]` — **the** gate, and the tier is a flag on it:
  - `just check` — the **affected tier**: the projects this change can reach,
    diffed against the base `scripts/nx-base.sh` derives — `NX_BASE` when it is a
    plain ref name or SHA that resolves (CI exports it with `nx-set-shas`), else
    the merge base with `origin/main`; any other `NX_BASE` fails closed, naming it.
    Must pass before any commit or PR.
  - `just check all` — the **broader tier**: the same targets over every project.
  Both tiers exclude the live projects and then run the workspace-level gates
  that span every stack — contract drift (`just contract-check`), release targets
  (`just release-targets-check`), module boundaries (`just boundaries-check`),
  workflow routing (`just workflows-check`), the base derivation's own test
  (`just base-check`) — and the Rust coverage floor.
- The **live** suites (`just test-live`, `just test-judge-api`, `just
  test-harness <id>`) are never part of `just check`: real model or API calls,
  credentials, network. `just test-oneharness` is deterministic but needs the
  `oneharness` binary, so it stays out too.

## The provider boundary

`skilltest` never talks to a model directly: every model, judge and
simulated-user call goes through the `Provider` trait in `skilltest-core`, whose
two backends — `OneharnessProvider` (default) and the JSON-lines
`CommandProvider` — and the tool-mocking seam are documented in
`crates/skilltest-core/AGENTS.md` and [`docs/protocol.md`](docs/protocol.md).
The deterministic gate always uses the bundled fake provider; only the live
projects reach a real harness.

## Invariants (non-negotiable)

- The quality gate is strict: no warnings-only mode. `clippy`, `ruff`, `ty`,
  `biome`, and `tsc` all fail the build on findings. A diagnostic is either an
  error or suppressed with a documented, tracked rationale.
- Validate all external / IO inputs at trust boundaries: config files, test-case
  YAML, skill frontmatter, and every provider response are parsed into typed
  models (`serde` in Rust, Pydantic in Python) before use. Never trust raw
  provider output. Case input is strict everywhere — a typo'd key must never
  silently apply a default. The one
  deliberate exception is the CLI's own `--format json` output inside the SDKs:
  its shape is guaranteed by the generated-model drift gate rather than
  re-validated at runtime, so SDKs may type-cast it after a JSON parse.
- The CLI's `--format json` output is a **stable contract** the SDKs depend on,
  and the Rust report types are its single source of truth. SDK models are
  **generated, never hand-written**: `just gen-contract` derives golden JSON
  Schemas (`schemas/`) from the types via `schemars`, then generates each SDK's
  models from the schemas (`datamodel-code-generator` for Python,
  `json-schema-to-typescript` for TypeScript — see `docs/schema.md`). The sync
  is enforced by `just contract-check` in the gate plus a Rust e2e test on the
  goldens. Changing the shape is a breaking change: change the Rust types, run
  `just gen-contract`, and commit the regenerated artifacts — then land it behind a
  `feat!:`/`BREAKING CHANGE` commit so the lockstep version moves on the next release
  (versions are never hand-bumped; see "Commits, releases, and merging").
- Keep the artifact portable across the supported platform matrix (Linux, macOS).
- Do not commit secrets, credentials, PII, or customer data. Real provider runs
  need API keys; those live in the environment, never in fixtures or config.
- No non-determinism in the gate: the LLM is always faked in tests.

## Scripts and output are context

- Every script you add should be quiet on success — a single line or nothing.
- On failure, print the exact error and a concrete suggested next action.
- The CLI follows the same rule: minimal human output on success, the exact
  problem plus a suggested action on stderr, distinct exit codes (see
  `crates/skilltest-core/src/exit.rs`).

## Tests are context engineering

- Tests are how you and future agents actually see this system behave, so invest
  in them deliberately.
- The e2e suites drive the **built** CLI the way users do — as a subprocess,
  asserting on exit codes and JSON — against the fake provider. When you touch
  the conversation loop, evals, or the JSON contract, extend an e2e journey
  (`crates/skilltest-cli-e2e`) rather than adding another narrow unit test.
- Every e2e suite must cover at least one happy path **and** one meaningful
  failure/recovery path (a failing eval, a malformed config, a missing provider).

## Commits, releases, and merging

**The whole repo shares one lockstep version, and conventional commits are the
source of truth — not the manifests, not the tag.** skilltest **releases on
merge**: PRs are squash-merged, so the PR title is the commit subject
semantic-release parses, and every ordinary push to `main` can cut a release
from exactly that commit.

**Where each gate tier runs (gate a given commit once).**

- **Pull request — the affected tier.** `ci.yml`'s `check` job runs `just check`
  against the merge base `nx-set-shas` exports, on Linux and macOS; with
  `pr-title` and `llmlint` these are the PR's contexts (`check (ubuntu-latest)`,
  `check (macos-latest)`, `pr-title`, `llmlint`). No live suite runs on a PR.
- **Merge to `main` — the broader tier, ahead of the release.** Because the
  merged commit *is* the released commit, the sweep runs here and only here: the
  same `check` job runs `just check all`, `ci.yml` calls every live e2e workflow
  (external contact promotes them out of the PR tier, unconditionally), and its
  `release` job calls `semantic-release.yml` only when the sweep (both OSes) and
  every live suite succeeded. A red sweep or live suite cuts no release.
- **The `chore(release): X.Y.Z` commit** differs from the swept commit only in
  version strings, so every push job in `ci.yml`, `bundle-smoke.yml` and
  `visual-docs.yml` skips it (job-level guards, not `[skip ci]`, which would also
  muzzle the tag workflows). The `vX.Y.Z` tag fires `release.yml` (binaries →
  GitHub Release) and `publish.yml` (registries), which build and publish but
  re-gate nothing.
- `scripts/check-workflow-routing.py` (`just workflows-check`, in every gate)
  simulates these events over the committed workflows and fails if the routing
  drifts.

**The release itself.** semantic-release reads the commits since the last
release, computes the next version, writes it into every manifest + lockfile +
`CHANGELOG.md` via `scripts/set-version.sh`, commits `chore(release): X.Y.Z` and
pushes tag `vX.Y.Z`. It never publishes; a registry hiccup must not block the
binary release, or vice versa.

- **Version policy (pre-1.0).** `feat:` and any `BREAKING CHANGE` → **minor**;
  `fix:`/`perf:`/`build:`/`refactor:`/`revert:` → **patch**; `chore`/`docs`/`ci`/
  `test` → no release. The major is held at `0` by the `releaseRules` in
  `.releaserc.json` (a breaking change bumps the minor, not to 1.0) until we
  consciously go 1.0 by removing that rule. PRs are **squash-merged**, so the PR
  **title** is the commit subject semantic-release parses — `pr-title.yml` enforces a
  conventional title so a bad subject can't silently skip or mis-size a release.
- **What a release publishes is declared** in `release-targets.toml`, which
  onevcs reads to hold another repository's plan until skilltest releases; its
  probe is `scripts/release-probe.sh`, and `scripts/check-release-targets.sh`
  fails `just check` when the declaration and `publish.yml` disagree. The
  target ids and short names are named by other repositories' plans, so a
  rename, addition or removal is a deliberate cross-repository change, never a
  side effect.
- **To cut a release:** merge a conventional-commit PR. That's it. The lockstep
  version is never hand-edited; if the JSON contract changed, run `just gen-contract`
  and commit the regenerated artifacts in the same PR — the version moves on its own at
  release time. (`scripts/set-version.sh X.Y.Z` exists to set/realign the baseline by
  hand; it also keeps the two cross-package pins in lockstep — the internal
  `skilltest-core` pin in `[workspace.dependencies]` and pytest's exact
  `skilltest-sdk==X.Y.Z` dependency.) A manual `v*` tag remains a re-publish escape
  hatch: it fires `publish.yml`/`release.yml` directly, and `publish.yml` skips any
  version already live, so re-fired tags are idempotent.
- **Why a PAT.** `semantic-release.yml` pushes the bump commit + tag with `RELEASE_PAT`
  (sourced from the `GH_TOKEN` gh-secrets item). The default `GITHUB_TOKEN` can neither
  push to protected `main` nor trigger the downstream tag workflows; a real PAT does
  both.
- **Tokens** come from the `gh-secrets.json` manifest — `CARGO_REGISTRY_TOKEN`,
  `PYPI_API_TOKEN`, `NPM_TOKEN` (publishing) and `RELEASE_PAT` (versioning). Each
  publish job targets the `release` GitHub Environment, so adding required reviewers
  under Settings → Environments → release gives a manual approval gate before any
  publish (no rules = no-op).
- **Per-registry specifics worth remembering.** crates.io must publish
  `skilltest-core` before `skilltest-cli` and wait for it to be indexed (cargo
  ≥1.66 blocks for this; the job polls as a backstop). The published `skilltest`
  crate ships a single binary — `skilltest-fake-provider` is gated behind the
  non-default `fake-provider` feature (`crates/skilltest-cli/AGENTS.md`). npm
  packages are scoped to the **`skill-test` org** and publish with `pnpm publish
  --access public`: pnpm rewrites `@skill-test/vitest`'s `workspace:*` dependency to the
  real version, and `--access public` is required for scoped packages (also set
  via `publishConfig`). PyPI builds from `[project]` metadata
  (`plugins/pytest/AGENTS.md`).

## Keeping the allowlist current

- The agent command allowlist lives in `.claude/settings.json`; the tool
  enforces it, so this file does not restate "follow the allowlist."
- Your job is to keep it current: when a new routine command becomes part of the
  normal build/test/release workflow, add it to the allowlist instead of
  re-approving it every session. Keep it narrow.

## Conventions

- Rust: the stable toolchain pinned in `rust-toolchain.toml`, `rustfmt` defaults,
  `clippy -D warnings`. Errors use `thiserror`; library errors become process
  exit codes in the CLI, never in the core.
- Python packages: Python 3.12+, `uv`, `ruff`, `ty`, `pytest`. Public API is
  re-exported from each package's `__init__.py`; everything else is internal.
- TS packages: `strict` TypeScript, `biome` (one root config) for lint+format,
  `vitest`, a `pnpm` workspace rooted at the repo. Public API is each package's
  `src/index.ts`.
- A new language gets one SDK under `sdks/<language>` — a CLI wrapper plus models
  **generated** from `schemas/` (`schemas/AGENTS.md`), nothing framework-specific.
  A new test framework gets one package under `plugins/<framework>` that builds on
  its language's SDK and re-exports it. Either gets a `project.json` with
  `type:`/`lang:` tags and a nested `AGENTS.md`.
- See `tests/AGENTS.md` for test-fixture conventions.

## After the main task: refine and hand off

After completing a requested task, propose only materially-helpful follow-ups
(scripts to automate a manual step, a constraint worth recording here, a fixture
that improves visibility). Skip busywork. If nothing is materially helpful, say
so and stop.
