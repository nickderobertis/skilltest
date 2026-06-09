# Development

## Prerequisites

`skilltest` is a Rust workspace with a Python plugin and a TypeScript plugin, so
`just` needs three toolchains on `PATH`:

- **Rust** (stable) with `cargo`, plus [`cargo-nextest`](https://nexte.st).
- [**uv**](https://docs.astral.sh/uv/) for the Python plugin.
- **Node** 22+ and [**pnpm**](https://pnpm.io) for the TypeScript plugin.

[`just`](https://github.com/casey/just) drives everything.

## The loop

```bash
just bootstrap   # cargo fetch + uv sync + pnpm install — works from a clean clone
just check       # the full gate: format, lint, type check, unit + e2e
just format      # auto-format all three stacks
just test        # fast Rust unit tests only
just test-e2e    # the cross-language e2e suites (builds the binaries first)
just upgrade     # bump deps across all stacks, then re-run check
```

`just check` is the single source of truth and is exactly what CI runs after a
clean `just bootstrap`. It is strict: `clippy`, `ruff`, `ty`, `biome`, and `tsc`
all fail the build on findings.

## How the e2e suites stay deterministic

The plugins shell out to the built `skilltest` binary; all suites point the
provider at `skilltest-fake-provider`, a deterministic reference implementation
of the [provider protocol](protocol.md). That exercises the entire pipeline —
arg parsing, YAML loading, the conversation loop, evals, the JSON contract, exit
codes — without a live model. A real model is never in the gate.

Running a real provider locally:

```bash
skilltest run cases/greet.yaml --provider oneharness -p claude-code -m claude-opus-4-8
```

## Smoke-testing a real provider

The gate is deterministic, so a live provider is never in it. There is one opt-in
smoke test, gated behind `#[ignore]` and an env var, to exercise a real provider
before a release:

```bash
SKILLTEST_LIVE_PROVIDER=oneharness \
  cargo test -p skilltest-cli --test live -- --ignored
```

Optional `SKILLTEST_LIVE_PLATFORM` / `SKILLTEST_LIVE_MODEL` override the
platform/model. It asserts the provider was reachable and returned a well-formed
report, not a specific pass/fail (a real model is non-deterministic).

## Releasing

1. `just audit` — `cargo deny` (advisories + licenses) before publishing.
2. Optionally run the live smoke test above against `oneharness`.
3. Push a tag `vX.Y.Z`. [`release.yml`](../.github/workflows/release.yml) builds
   the `skilltest` binary on native runners for Linux and macOS
   (x86_64 + aarch64) and uploads each as `skilltest-<target>.tar.gz` plus a
   `.sha256` to the GitHub Release.
4. [`scripts/install.sh`](../scripts/install.sh) consumes those assets; verify it
   end-to-end after the first release.

The `--format json` output of `run` and `validate` is a stable contract the
plugins parse; changing its shape means updating the Rust types, the Pydantic
models, and the Zod schema together, and bumping versions.
