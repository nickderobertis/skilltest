# crates/skilltest-cli-e2e — the CLI's binary e2e tier

Nx project `skilltest-cli-e2e` (`type:e2e`, `lang:rust`), a `publish = false`
crate holding only `tests/`. It drives the **built** `skilltest` binary the way a
user does — as a subprocess, asserting on exit codes and JSON — against the
deterministic `skilltest-fake-provider`. Only the model is faked.

- **What it proves.** `pnpm exec nx run skilltest-cli-e2e:test` — which never
  starts before `skilltest-cli:build` has produced the binary — runs `e2e.rs`
  (the representative journeys: single/multi-turn, evals, mocks/spies,
  streaming, the JSON contract against the `schemas/` goldens and the
  kitchen-sink case golden), `cli_errors.rs` (error classification, exit codes,
  hints), `fake_provider.rs` (the reference provider's protocol, malformed input
  included), plus two repository-consistency gates: `pins.rs` (the oneharness
  pin across the installer, recipe and both SDKs) and `release_assets.rs` (what
  `scripts/set-version.sh` rewrites).
- **Depends on.** `skilltest-cli` (the binary under test), `skilltest-core`
  (`e2e.rs` constructs the kitchen-sink case in Rust) and `skilltest-contract`
  (the goldens). Nothing may depend on it, so it is reachable only from a change
  to those or to itself.

## Rules for this crate

- Resolve binaries with `common::built_bin` (beside the test executable, in
  `<target>/<profile>/`). A crate other than the binary's own gets no
  `CARGO_BIN_EXE_*`; a missing binary fails loudly with the build command.
- Every suite keeps a happy path **and** a meaningful failure/recovery path (a
  failing eval, malformed config, a missing or misbehaving provider). Extend a
  journey here when you touch the conversation loop, evals or the JSON contract,
  rather than adding another narrow unit test in the core.
- Never `#[ignore]` a test here: these suites are the deterministic gate. They
  also run inside `just coverage` (`cargo llvm-cov nextest --workspace`), which
  builds the CLI's binaries instrumented in its own target dir; that is how the
  95% line floor measures the CLI through these journeys. The crate has no
  library target, so it adds no lines of its own to the measurement.
