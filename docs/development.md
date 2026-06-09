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

## Releasing

`just audit` runs `cargo deny` (advisories + licenses) before publishing
binaries. The `--format json` output of `run` and `validate` is a stable
contract the plugins parse; changing its shape means updating the Rust types, the
Pydantic models, and the Zod schema together, and bumping versions.
