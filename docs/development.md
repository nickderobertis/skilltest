# Development

## Prerequisites

`skilltest` is a Rust workspace plus, per language, an SDK package and a
test-framework package (Python: `sdks/python` + `plugins/pytest`; TypeScript:
`sdks/typescript` + `plugins/vitest` in a pnpm workspace rooted at the repo), so
`just` needs three toolchains on `PATH`:

- **Rust** (stable) with `cargo`, plus [`cargo-nextest`](https://nexte.st).
- [**uv**](https://docs.astral.sh/uv/) for the Python packages.
- **Node** 22+ and [**pnpm**](https://pnpm.io) for the TypeScript packages.

[`just`](https://github.com/casey/just) drives everything, as a thin wrapper
over [nx](https://nx.dev): each package has a `project.json` with its targets,
and the default recipes run only the projects **affected** by your change
(`just check-all` forces all). nx itself is installed by `just bootstrap`.

## The loop

```bash
just bootstrap   # pnpm install (nx + TS workspace) + cargo fetch + uv sync — works from a clean clone
just check       # contract drift gate + the full gate (format, lint, types, unit + e2e) over affected projects
just check-all   # the same gate across every project
just format      # auto-format all three stacks
just test        # fast Rust unit tests only
just test-e2e    # the cross-language e2e suites (nx builds prerequisites first)
just gen-contract # regenerate schemas/ + the generated SDK models from the Rust types
just graph       # open the interactive nx project graph
just upgrade     # bump deps across all stacks, then re-run check-all
```

`just check` is the single source of truth and is exactly what CI runs after a
clean `just bootstrap` (CI uses `nrwl/nx-set-shas` to pick the affected base).
It is strict: `clippy`, `ruff`, `ty`, `biome`, and `tsc` all fail the build on
findings.

## How the e2e suites stay deterministic

The SDKs shell out to the built `skilltest` binary; all suites point the
provider at `skilltest-fake-provider`, a deterministic reference implementation
of the [provider protocol](protocol.md). That exercises the entire pipeline —
arg parsing, YAML loading, the conversation loop, evals, the JSON contract, exit
codes — without a live model. A real model is never in the gate.

Running a real provider locally:

```bash
skilltest run cases/greet.yaml --provider oneharness -p claude-code -m claude-opus-4-8
```

## Live tests against real oneharness

The gate is deterministic (fake provider), so real model calls are never in it.
The live suite (`crates/skilltest-cli/tests/live.rs`) drives the skilltest CLI
through **real** oneharness + a real harness, and is `#[ignore]`d so it only runs
when you ask. Build [oneharness](https://github.com/nickderobertis/oneharness),
then:

```bash
SKILLTEST_ONEHARNESS_BIN=/path/to/oneharness/target/debug/oneharness \
  cargo test -p skilltest-cli --test live -- --ignored
```

It uses near-deterministic fixtures (`tests/fixtures/live/`) — a skill that always
replies "pong", and a two-turn echo skill — so a real judge has an unambiguous
verdict. It covers `respond` (via oneharness's `--system` for the skill and
`--resume` for multi-turn against the real harness session), boolean + numeric
`judge`, the simulated-user multi-turn loop, and asserts that the normalized
`usage` totals flow into each `CaseRun` and the report `summary`. Optional
`SKILLTEST_LIVE_PLATFORM` (default `claude-code`) and `SKILLTEST_LIVE_MODEL`
(default `haiku`) override the harness/model.

## Releasing

Releases are **automatic and lockstep** — you do not bump versions or push tags by
hand. Merge a Conventional-Commits PR to `main` and
[`semantic-release.yml`](../.github/workflows/semantic-release.yml) computes the next
0.x version from the commit history, writes it into every manifest + lockfile +
`CHANGELOG.md` via [`scripts/set-version.sh`](../scripts/set-version.sh), commits
`chore(release): X.Y.Z`, and pushes tag `vX.Y.Z`. That tag then fires
[`release.yml`](../.github/workflows/release.yml) (binaries) and
[`publish.yml`](../.github/workflows/publish.yml) (registries). See the "Publishing"
section of [`AGENTS.md`](../AGENTS.md) for the version policy and the PAT.

- Since PRs are squash-merged, the **PR title** is the commit subject semantic-release
  parses; `pr-title.yml` enforces a conventional title.
- `just audit` (`cargo deny`, advisories + licenses) and the live suite above are
  worth running on a release-bearing PR before merge.
- A manual `vX.Y.Z` tag remains a re-publish escape hatch (fires `release.yml` /
  `publish.yml` directly; `publish.yml` skips anything already live).
- One-time rollout: sync `RELEASE_PAT`, push a `v0.0.0` seed tag (guarded so it never
  publishes) to anchor the first release at `0.1.0`, and add `pr-title` to the branch's
  required checks.

### Bundled binaries (the SDKs ship the CLI)

`pip install skilltest-sdk` and `pnpm add @skill-test/sdk` need no separate binary
install: each SDK bundles the CLI and the package manager picks the right build for
the host automatically.

- **npm.** The CLI ships in four `os`/`cpu`-scoped packages,
  `@skill-test/cli-{linux,darwin}-{x64,arm64}`, declared as the SDK's
  `optionalDependencies`; npm/pnpm install only the one matching the host. Each lives
  under [`sdks/typescript/platforms`](../sdks/typescript/platforms) as a workspace
  package whose `bin/` is git-ignored and filled at publish time.
- **PyPI.** `skilltest-sdk` publishes a platform wheel per target with the binary at
  `skilltest_sdk/_bin/skilltest`, plus a pure (`py3-none-any`) wheel and an sdist;
  `pip` prefers the matching platform wheel and falls back to the pure one on an
  unsupported platform.
- **Resolution.** Both runners resolve the binary most-explicit-first: an explicit
  `bin` arg, then `$SKILLTEST_BIN`, then the bundled binary, then `skilltest` on
  `PATH`. A source checkout bundles nothing, so it falls through to `$SKILLTEST_BIN`
  (how the e2e suites reach `target/debug/skilltest`).

`publish.yml` drives this: a `binaries` matrix builds the CLI per target, then the npm
job stages each into its platform package (`scripts/stage-npm-binary.sh`) and the PyPI
job assembles the wheels (`scripts/build-python-dist.sh`). The platform packages
publish **before** the SDK, because its `optionalDependencies` pin their exact
versions; `set-version.sh` keeps every version in lockstep. The platform packages
publish via `npm` (which preserves the binary's executable bit) while the SDK + vitest
publish via `pnpm` (which rewrites their `workspace:*` deps).

This path has its own gate. The normal `just check` points the SDKs at
`$SKILLTEST_BIN`, so it never runs the bundled binary;
[`bundle-smoke.yml`](../.github/workflows/bundle-smoke.yml) closes that gap. On every
PR and push to `main`, for each of the four targets, it builds the CLI, installs the
publish-shape package with the binary bundled into a fresh consumer project, and runs a
case through the **plugin** with `SKILLTEST_BIN` unset — so a pass can only come from
the bundle (`scripts/smoke-{python,npm}-bundle.sh`). It drives the fake provider, so
it stays deterministic.

The `--format json` output of `run` and `validate` is a stable contract the
SDKs parse; the SDK models are generated from the Rust types, so changing the
shape means changing the types, running `just gen-contract`, and committing the
regenerated artifacts behind a `feat!:`/`BREAKING CHANGE` commit so the lockstep
version moves (see "Output contract" in [schema.md](schema.md)).
