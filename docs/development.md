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

The `--format json` output of `run` and `validate` is a stable contract the
SDKs parse; the SDK models are generated from the Rust types, so changing the
shape means changing the types, running `just gen-contract`, and committing the
regenerated artifacts behind a `feat!:`/`BREAKING CHANGE` commit so the lockstep
version moves (see "Output contract" in [schema.md](schema.md)).
