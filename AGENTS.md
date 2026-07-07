# AGENTS.md

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

This repo is composed from the `create-repo` skill's reference axes rather than
a single template. What was pulled in, and why:

- **Product shape — CLI (`shapes/cli.md`) + library (`shapes/library.md`).** The
  shipped artifact is a compiled **Rust CLI** (`skilltest`); `skilltest-core` is
  a reusable **library** and the source of truth for the JSON contract. The CLI
  guidance drives "test the *built* binary as a subprocess, e2e in the gate"; the
  library guidance drives the stable, documented public API in `skilltest-core`.
- **Language — Rust (`languages/rust.md`) + its CLI intersection
  (`intersections/rust-cli.md`).** These set the toolchain and gates: stable
  Rust, `rustfmt` + `clippy -D warnings` (strict, no warnings-only mode), `cargo
  nextest` for unit/integration + a separate binary e2e target both in `just
  check`, `cargo llvm-cov --fail-under-lines` for coverage, and `cargo deny` +
  `cargo machete` for supply chain (the `just audit` recipe; run before
  publishing binaries). The SDK languages — **Python** (`languages/python.md`:
  uv/ruff/ty/pytest) and **TypeScript** (`languages/typescript.md`: biome/tsc/
  vitest) — are pulled in for the thin per-language SDKs and framework packages,
  each running its own native toolchain.
- **Cross-cutting — `ci.md` (always)** and **`monorepo.md` (applies).** `ci.md`
  gives clean-checkout → `just bootstrap` → `just check` on a Linux/macOS matrix,
  the live/integration test tier kept out of the gate in its own fork-safe
  workflows (the `e2e-*` and `*-api` workflows), the install-path smoke proof
  (`bundle-smoke.yml`), and the merge model in "Publishing" / "Repository
  settings" below. `monorepo.md` applies because the repo holds **>1 deliverable
  in >1 language** (a Rust workspace + Python and TypeScript SDKs + per-framework
  packages): it is orchestrated by **Nx** (root `just` recipes delegate to `nx
  affected`/`run-many`; per-project `project.json` targets; lockstep versioning
  through `scripts/set-version.sh`; the generated cross-language contract in
  `schemas/` drift-checked by `just contract-check`).
- **Excluded and why.** `shapes/nextjs.md`, `shapes/web-app.md`,
  `shapes/skills-repo.md`, and `shapes/asdf-plugin.md` — there is no web app, and
  while skilltest *tests* skills it is not itself a skills repo. The
  `intersections/python-cli.md` reference does not apply: the Python package is a
  thin **SDK/library** wrapping the Rust CLI, not its own console entry point. No
  `bash.md` shape — the `scripts/*.sh` are build/release glue (kept
  orchestrator-independent per `monorepo.md`), not a shipped Bash artifact.

### Coverage and e2e (the gate's depth)

- **Coverage — enforced, default bar (95% lines).** `just coverage` (wired into
  `just check` and `check-all`) runs `cargo llvm-cov nextest --workspace
  --features fake-provider --fail-under-lines 95` and **fails the gate below 95%
  line coverage** on the artifact's Rust core (`skilltest-core` + the `skilltest`
  CLI, including the binary e2e suite and the bundled fake provider). The current
  figure is ~97% lines. Coverage runs over the **whole Rust workspace** (not
  nx-affected) on purpose: the binary is the published artifact, so its coverage
  floor is proven on every gate run, not only when a Rust file changed. The
  thin Python/TS SDKs are proven by their own `nx test`/`test-e2e` targets and
  the bundled-binary install smoke; the 95% line bar is enforced on the Rust core
  where the behavior lives.
- **E2E — real, in the gate.** The deterministic e2e suites drive the **built**
  CLI as a subprocess against the `skilltest-fake-provider` (only the model is
  faked): `crates/skilltest-cli/tests/e2e.rs` plus `cli_errors.rs` (the CLI's
  error-classification/dispatch paths) and `fake_provider.rs` (the reference
  provider's protocol), wired into `just check` via the `test-e2e` target and the
  coverage run. Each suite covers a happy path **and** ≥1 failure/recovery path
  (failing eval, malformed config, missing provider, classified provider
  errors). The **live** tier that needs real harnesses/APIs stays out of the gate
  (non-deterministic, credentialed) and runs in the per-harness `e2e-*` /
  `e2e-judge-api` workflows — it still compiles in the normal build (gated at
  runtime via `--ignored`), per `ci.md`'s live-tier rule. See "The provider
  boundary" and `docs/e2e.md`.

## Layout

| Path | What |
| --- | --- |
| `crates/skilltest-core` | Library: config, skill model + validation, test-case model, provider protocol, evals, runner, report. The stable Rust API the CLI builds on, and the source of truth for the JSON contract. |
| `crates/skilltest-cli` | The `skilltest` binary (clap), including `skilltest schema` (emits the contract's JSON Schemas). `run` ingests cases from positional YAML `PATH`s **or** `--case-json <FILE>` — a JSON case object/array (the delivery channel for a case built in an SDK; `skill` resolves relative to CWD, not a file). Also carries `skilltest-fake-provider`, a deterministic reference provider used by the e2e suite — a second `[[bin]]` gated behind the non-default `fake-provider` feature so a published `cargo install` ships only `skilltest`; the nx `build`/`lint` targets enable the feature, release builds don't. |
| `sdks/python` | `skilltest-sdk`: the Python SDK — runs the CLI as a subprocess and parses its JSON contract into Pydantic models (`run_skill`), plus an opt-in async streaming API (`stream_skill` → `SkillStream`, an `async for` of tool events that `break`s to short-circuit) and `tool_calls`/`ToolEvent` for tool-event analysis. `run_skill`/`stream_skill` take a YAML path **or** a code-defined `TestCase` (the `case.py` builders — `TestCase`/`user`/`boolean`/`numeric`/`called`/`not_called`, reusing the `mock.py` builders; the builders construct the **generated** `_case.py` models from the input contract, so the payload cannot drift from the Rust parse — delivered via `--case-json`). No framework code. Ships a per-target **platform wheel** that bundles the CLI at `skilltest_sdk/_bin/skilltest` (plus a pure-wheel/sdist fallback), so `pip install` needs no separate binary step; the runner resolves the bundled binary, falling back to `$SKILLTEST_BIN`/`PATH`. |
| `sdks/typescript` | `@skill-test/sdk`: the TypeScript SDK — same wrapper with generated type declarations (`runSkill`), plus the matching async streaming API (`streamSkill` → `SkillStream`, a `for await` of tool events that `break`s to short-circuit) and `toolCalls`/`ToolEvent`. `runSkill`/`streamSkill` take a YAML path **or** a code-defined case (the `case.ts` builders — `testCase`/`user`/`boolean`/`numeric`/`called`/`notCalled`, typed against the **generated** `src/generated/case.ts` input-contract types). No framework code. Bundles the CLI via the per-platform `@skill-test/cli-*` packages (see `sdks/typescript/platforms`), declared as `optionalDependencies` so `pnpm add` pulls only the matching host's binary; the runner resolves it, falling back to `$SKILLTEST_BIN`/`PATH`. |
| `sdks/typescript/platforms/cli-*` | The four binary-carrier npm packages (`@skill-test/cli-{linux,darwin}-{x64,arm64}`), each `os`/`cpu`-scoped with a git-ignored `bin/` filled at publish time. Workspace members pinned by the SDK via `workspace:*`; `scripts/set-version.sh` keeps their versions in lockstep. |
| `plugins/pytest` | `skilltest-pytest`: pytest collection of `*.skilltest.yaml` cases, built on (and re-exporting) `skilltest-sdk`. |
| `plugins/vitest` | `@skill-test/vitest`: `skillTest`/`discover` vitest helpers, built on (and re-exporting) `@skill-test/sdk`. |
| `schemas/` | Golden JSON Schemas (draft-07), generated from the Rust types and the source every SDK's models are generated from: the **output contract** (`report`/`validation`, the `--format json` shapes) and the **input contract** (`case`, the test-case shape — `--case-json`/YAML). The input side is additionally pinned by the kitchen-sink golden (`tests/fixtures/contract/case_kitchen_sink.json`), which the Rust construction and both SDKs' case builders must all serialize to exactly. Regenerated by `just gen-contract`. |
| `tests/fixtures` | Sample skills and YAML test cases shared by the e2e suites. |
| `docs/` | The provider protocol, config/test-case schema, and live-e2e (`docs/e2e.md`) references. |
| `scripts/install.sh` | Installs a prebuilt `skilltest` from a GitHub Release (verifies checksum). |
| `scripts/stage-npm-binary.sh` | Stages a built binary into its `@skill-test/cli-*` package's `bin/` for packing/publishing. |
| `scripts/build-python-wheel.sh`, `scripts/build-python-dist.sh` | Build a platform-tagged `skilltest-sdk` wheel that bundles the CLI (the former, one target); assemble the full dist — every platform wheel + pure wheel + sdist (the latter). |
| `scripts/smoke-python-bundle.sh`, `scripts/smoke-npm-bundle.sh` | Bundle smoke: install the publish-shape package with the binary bundled into a fresh consumer project and run a case through the plugin with `SKILLTEST_BIN` unset, so a pass can only come from the bundled binary. Driven per platform by `bundle-smoke.yml`. |
| `scripts/install-oneharness.sh` | Installs the prebuilt `oneharness` the live e2e drives (verifies checksum). |
| `scripts/e2e-lib.sh`, `scripts/e2e-harness.sh` | Live, per-harness e2e: drive the built CLI against a *real* harness through oneharness. See `docs/e2e.md`. |
| `scripts/set-version.sh` | Writes one lockstep version into all six manifests + the four `@skill-test/cli-*` platform packages + every lockfile + the two cross-package pins. Invoked by semantic-release each release; idempotent and runnable by hand. |
| `gh-secrets.json` | Declarative secret manifest, synced from Bitwarden to the GitHub repo + a gitignored local `.env` via `gh-secrets manifest sync`. |
| `.github/workflows/semantic-release.yml` | Lockstep versioning: on merge to `main`, computes the next version from conventional commits, writes it everywhere via `scripts/set-version.sh`, commits + tags `v*`. Never publishes. See "Publishing". |
| `.github/workflows/release.yml` | Tag-triggered cross-platform binary build + checksums for the GitHub Release `scripts/install.sh` consumes (fired by the `v*` tag semantic-release pushes). |
| `.github/workflows/publish.yml` | Tag-triggered registry publish (crates.io, PyPI, npm) in dependency order; skips any version already live, so re-fired tags are idempotent. A `binaries` matrix builds the CLI per target so the npm/PyPI jobs can bundle it into the per-platform packages/wheels. See "Publishing". |
| `.github/workflows/pr-title.yml` | Enforces a Conventional-Commits PR title (the squash-merge subject semantic-release parses). |
| `.github/workflows/bundle-smoke.yml` | On PR + push to `main`, proves the SDKs run the **bundled** CLI (not `$SKILLTEST_BIN`): builds the CLI per target, installs the publish-shape packages, and runs a case through each plugin on a native runner. Covers linux x64/arm64 + darwin arm64; the Intel-macOS (`macos-13`) runner is skipped here (unreliable queue) though that binary is still built/published. |
| `.github/workflows/e2e-<id>.yml` | One live per-harness e2e each (claude, codex, goose, opencode, cursor, crush, qwen, copilot), gated to the canonical repo and non-fork PRs. |
| `.github/workflows/e2e-judge-api.yml` | Live e2e for the **direct-API judge** (`ApiJudgeProvider`): calls the real Anthropic + OpenAI APIs (strict-JSON structured outputs, verdict parsing, usage), needs `ANTHROPIC_API_KEY`/`OPENAI_API_KEY`, gated to the canonical repo and non-fork PRs. No oneharness/harness CLI. |
| `nx.json` | Nx workspace config: the `affected` base, cache/input rules, and per-target defaults (`dependsOn`, caching). |
| `<project>/project.json` | One per package (`crates/skilltest-{core,cli}`, `sdks/{python,typescript}`, `plugins/{pytest,vitest}`): the package's nx targets and its `implicitDependencies` edge in the graph. |

## Command surface

Use the `just` recipes; do not hand-roll equivalent commands. `just` is a thin
wrapper over **nx** — each recipe drives the per-package targets in the
`project.json` files. The project graph is `skilltest-core` ← `skilltest-cli` ←
`{skilltest-sdk, @skill-test/sdk}` ← `{skilltest-pytest, @skill-test/vitest}`
(the SDKs shell out to the built CLI; each framework package builds on its
language's SDK), so nx builds prerequisites in order and, for the default
recipes, runs **only the [affected](https://nx.dev/ci/features/affected)
projects** (diffed against the `main` base in `nx.json`). A TS-only change
never spends time on the Rust or Python suites; a core change fans out to the
CLI, both SDKs, and both framework packages. The one workspace-level exception
is the contract drift gate (`just contract-check`), which spans every stack and
always runs as part of `just check`.

- `just bootstrap` — set up from a clean clone (root `pnpm install` for nx and
  the whole TS workspace, then `cargo fetch` + `uv sync` in both Python
  packages).
- `just check` — the contract drift gate plus the full quality gate (format,
  lint, type check, unit + e2e) over the **affected** projects. Must pass
  before any commit or PR.
- `just check-all` — the same gate forced across **every** project (`nx run-many`).
  Use when you need the whole matrix regardless of what changed.
- `just test` / `just lint` / `just format` / `just typecheck` / `just build` —
  individual gate steps (affected; `format` runs across all projects).
- `just test-e2e` — the cross-language end-to-end suites; nx builds
  prerequisites first via the graph (CLI before SDKs, SDKs before framework
  packages).
- `just gen-contract` — regenerate the contract artifacts: golden JSON Schemas
  in `schemas/` from the Rust report types, then every SDK's generated models
  from the schemas. Run it whenever the report types change; `just
  contract-check` fails while anything is stale.
- `just graph` — open the interactive nx project graph.
- `just upgrade` — upgrade dependencies across nx + all three stacks, then
  `just check-all`.
- `just install-oneharness` / `just test-live` / `just test-harness <id>` — the
  **opt-in live e2e** against a real harness (never in `just check`; needs
  `oneharness`, a harness binary, a synced secret, and network). See `docs/e2e.md`.

`just` needs `cargo` (+ `cargo-nextest`), `uv`, and `node`/`pnpm` on `PATH`; nx
itself is a root dev-dependency installed by `just bootstrap`. CI installs the
toolchains, then uses `nrwl/nx-set-shas` so `just check` gates only the affected
projects per PR. Locally, install the toolchains once (see `docs/development.md`).

## The provider boundary

`skilltest` never talks to a model directly. The `Provider` trait
(`provider.rs`) has two real backends; see [`docs/protocol.md`](docs/protocol.md).

- **`OneharnessProvider` (default).** Targets
  [`oneharness`](https://github.com/nickderobertis/oneharness) **v0.3.6+** and
  uses five of its normalized features directly so skilltest can stop string-
  munging: `--system <skill instructions>` carries the skill as a real system
  prompt; `--resume <session_id>` continues a real harness session for the
  multi-turn loop on harnesses where `supports_resume` is true (claude-code,
  opencode, cursor today — others fall back to inlining the transcript);
  `--events` surfaces normalized tool events (`{kind, name, input, output,
  index}`) skilltest lifts onto each assistant turn (`Message.events`) so
  consumers can assert on *what the skill did*, not just its text;
  `results[*].usage` is aggregated into the report (`{input_tokens,
  output_tokens, cost_usd}`); and `results[*].failure_kind` (`auth` /
  `rate_limit` / `model_not_found` / `quota`) is surfaced through `Error::Provider
  { kind }` so the CLI gives a pointed hint. skilltest passes **no `--mode`**, so
  oneharness's own default approval mode applies (v0.3.0+ normalized `--mode`, a
  breaking change from pre-0.3 allow-everything); users set `bypass` etc. via
  oneharness config (`ONEHARNESS_MODE`), keeping approval policy in one place.
  A streaming variant (`respond_streaming`, `oneharness run --stream`) forwards
  tool events live and, on a sink `ControlFlow::Break`, kills the oneharness child
  to short-circuit a bad run; the buffered `respond` (`--compact`) is the default.
  Evals and the simulated user run on a fixed `judge_harness`, independent of the
  harness under test. Verdict JSON is parsed tolerantly (real models wrap it in
  prose/fences) and type-checked.
- **`CommandProvider`.** A small JSON-lines protocol (one request object on
  stdin, one response on stdout, per op) backing the bundled
  `skilltest-fake-provider` and any custom provider. Custom providers may
  optionally emit `usage`, `session_id`, and `events` on `respond` to participate
  in cost reporting, tool-event analysis, and stateful multi-turn — and may
  honor the `mocks` request block (returning `mock_calls`) to participate in
  tool mocking; ignoring the block while it's present is a loud provider error.

**Tool mocking/spying**: `skilltest-core::mock` compiles a case's `mocks:` (and
the CLI's `--mocks`/`--spy`, the SDKs' delivery path) to the oneharness ruleset;
`OneharnessProvider` passes `run --mock-rules`/`--spy-file` per skill turn
(never to the judge) and parses the spy JSONL into `CaseRun.mock_calls`
(original pre-rewrite inputs + verdicts). Two matching engines on purpose:
the hook-side `oneharness mock`, mirrored by `mock::decide` for the fake
provider; `just test-oneharness` (real binary + the `fake-claude.sh` shim,
hermetic) and the live e2e are the drift alarms. Anything inexpressible or
unresolvable errors loudly — never a vacuous pass.

The fake provider is why the whole pipeline is testable without a live model: it
implements the protocol deterministically, so the default e2e suites exercise the
real argument parsing, YAML loading, conversation loop, eval logic, exit codes,
and JSON output — everything except the non-deterministic model. The
`OneharnessProvider` path is proven separately by the opt-in live tests
(`crates/skilltest-cli/tests/live.rs`, the deep claude-code suite) plus the
generic per-harness smoke (`scripts/e2e-harness.sh`), which run against real
oneharness + a real harness and are never in the gate. skilltest carries the
skill via `--system`; **oneharness v0.2.1+** delivers that to every harness (a
native flag for claude-code/goose, prepended to the prompt otherwise) and
**v0.2.37+** extracts every harness's reply text natively (OpenCode's nested JSONL
included, via `text_source: json:opencode-parts`). Combined with two skilltest-
side provider rules — **omit `--model`** when it is unspecified so the harness uses
its own default/env model, and fall back to a harness's **raw stdout** as
defense-in-depth for oneharness's "text may be null" contract (no harness relies on
it today) — the **entire matrix is live-green and in CI: claude-code, codex, goose,
opencode, cursor, crush, qwen, copilot.** Each has a per-harness workflow
(`.github/workflows/e2e-<id>.yml`); a harness is only added once validated, else
it stays a loud skip. `docs/e2e.md` holds the full matrix (models, per-harness
delivery/extraction, the qwen gpt-5 gotcha), the secrets flow (`gh-secrets.json`),
and the runbook for adding a harness.

## Invariants (non-negotiable)

- The quality gate is strict: no warnings-only mode. `clippy`, `ruff`, `ty`,
  `biome`, and `tsc` all fail the build on findings. A diagnostic is either an
  error or suppressed with a documented, tracked rationale.
- Validate all external / IO inputs at trust boundaries: config files, test-case
  YAML, skill frontmatter, and every provider response are parsed into typed
  models (`serde` in Rust, Pydantic in Python) before use. Never trust raw
  provider output. Case input is strict **everywhere**: unknown fields are
  rejected even inside evals, the `user` block, and the map forms of
  `stub`/`deny`/field predicates (the `Eval` enum uses newtype variants around
  `deny_unknown_fields` structs because serde cannot deny on an internally
  tagged enum directly) — a typo'd key must never silently apply a default. The one deliberate exception is the CLI's own `--format
  json` output inside the SDKs: its shape is guaranteed by the generated-model
  drift gate rather than re-validated at runtime, so SDKs may type-cast it
  after a JSON parse (Python still gets full validation for free via the
  generated Pydantic models).
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
  (versions are never hand-bumped; see "Publishing").
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
  rather than adding another narrow unit test.
- Every e2e suite must cover at least one happy path **and** one meaningful
  failure/recovery path (a failing eval, a malformed config, a missing provider).

## Publishing

**The whole repo shares one lockstep version, and conventional commits are the
source of truth — not the manifests, not the tag.** Releases are automatic:
`semantic-release.yml` runs on every merge to `main`, reads the Conventional-Commits
history since the last release, computes the next version, writes it into all six
manifests + every lockfile + `CHANGELOG.md` (via `scripts/set-version.sh`), commits
`chore(release): X.Y.Z` to `main`, and pushes tag `vX.Y.Z`. **semantic-release never
publishes** — the tag it pushes triggers the two decoupled tag workflows: `release.yml`
(CLI binaries → GitHub Release) and `publish.yml` (the six packages → registries). A
registry hiccup must not block the binary release, or vice versa.

- **Version policy (pre-1.0).** `feat:` and any `BREAKING CHANGE` → **minor**;
  `fix:`/`perf:`/`build:`/`refactor:`/`revert:` → **patch**; `chore`/`docs`/`ci`/
  `test` → no release. The major is held at `0` by the `releaseRules` in
  `.releaserc.json` (a breaking change bumps the minor, not to 1.0) until we
  consciously go 1.0 by removing that rule. PRs are **squash-merged**, so the PR
  **title** is the commit subject semantic-release parses — `pr-title.yml` enforces a
  conventional title so a bad subject can't silently skip or mis-size a release.
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
  both. The release commit deliberately omits `[skip ci]` (which could also muzzle the
  tag workflows); instead `semantic-release.yml` guards against its own
  `chore(release):` commits so it doesn't re-run.
- **Tokens** come from the `gh-secrets.json` manifest — `CARGO_REGISTRY_TOKEN`,
  `PYPI_API_TOKEN`, `NPM_TOKEN` (publishing) and `RELEASE_PAT` (versioning). Each
  publish job targets the `release` GitHub Environment, so adding required reviewers
  under Settings → Environments → release gives a manual approval gate before any
  publish (no rules = no-op).
- **Per-registry specifics worth remembering.** crates.io must publish
  `skilltest-core` before `skilltest-cli` and wait for it to be indexed (cargo
  ≥1.66 blocks for this; the job polls as a backstop). The published `skilltest`
  crate ships a single binary — `skilltest-fake-provider` is gated behind the
  non-default `fake-provider` feature (see the layout note). npm packages are
  scoped to the **`skill-test` org** and publish with `pnpm publish --access
  public`: pnpm rewrites `@skill-test/vitest`'s `workspace:*` dependency to the
  real version, and `--access public` is required for scoped packages (also set
  via `publishConfig`). PyPI builds from `[project]` metadata, so the editable
  `[tool.uv.sources]` path in `skilltest-pytest` never reaches the wheel.

## Keeping the allowlist current

- The agent command allowlist lives in `.claude/settings.json`; the tool
  enforces it, so this file does not restate "follow the allowlist."
- Your job is to keep it current: when a new routine command becomes part of the
  normal build/test/release workflow, add it to the allowlist instead of
  re-approving it every session. Keep it narrow.

## Conventions

- Rust: stable toolchain, `rustfmt` defaults, `clippy -D warnings`. Errors use
  `thiserror`; the boundary between library errors and process exit codes lives
  in the CLI, not the core.
- Python packages: Python 3.12+, `uv`, `ruff`, `ty`, `pytest`. Public API is
  re-exported from each package's `__init__.py`; everything else is internal.
  `skilltest-pytest` consumes `skilltest-sdk` via a `[tool.uv.sources]` path
  source in dev and a version range when published.
- TS packages: `strict` TypeScript, `biome` (one root config) for lint+format,
  `vitest`, a `pnpm` workspace rooted at the repo. Public API is each package's
  `src/index.ts`. `@skill-test/vitest` consumes `@skill-test/sdk` as a
  `workspace:*` dependency; build the SDK before typechecking/testing the
  plugin (the `just` recipes do).
- A new language gets one SDK under `sdks/<language>`: a CLI wrapper plus
  models **generated** from `schemas/` (add the generator invocation and its
  output paths to `scripts/gen-contract.sh`; prefer the language's standard
  JSON-Schema-to-types generator, with quicktype as the fallback), nothing
  framework-specific. A new test framework gets one package under
  `plugins/<framework>` that builds on its language's SDK and re-exports it.
- See `tests/AGENTS.md` for test-fixture conventions.

## After the main task: refine and hand off

After completing a requested task, propose only materially-helpful follow-ups
(scripts to automate a manual step, a constraint worth recording here, a fixture
that improves visibility). Skip busywork. If nothing is materially helpful, say
so and stop.
