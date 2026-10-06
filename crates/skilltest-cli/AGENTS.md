# crates/skilltest-cli — the `skilltest` binary

Nx project `skilltest-cli` (`type:app`, `lang:rust`). The shipped artifact: the
clap CLI (`run`, `validate`, `init`, `schema`), plus `skilltest-fake-provider`,
the deterministic reference provider the e2e suites drive.

- **What it proves.** Its `test` target runs the unit suites in `src/` (`kind(lib)
  | kind(bin)`). The binary is proven end to end by the separate
  `skilltest-cli-e2e` project (`crates/skilltest-cli-e2e/AGENTS.md`), and by the
  SDKs' e2e suites, which shell out to it.
- **Depends on.** `skilltest-core`. Depended on by the CLI e2e project, both
  SDKs, and the live projects.
- **Run.** `pnpm exec nx run skilltest-cli:<build|test|lint|format-check|format>`.
  `build` (`cargo build -p skilltest-cli --features fake-provider`) is what every
  binary-driving suite waits on.

## Rules for this crate

- `run` ingests cases from positional YAML `PATH`s **or** `--case-json <FILE>` —
  a JSON case object/array, the delivery channel for a case built in an SDK
  (`skill` then resolves relative to the CWD, not to a file).
- `skilltest schema <report|validation|case|error>` emits the contract's JSON
  Schemas; `scripts/gen-contract.sh` (project `skilltest-contract`) runs it.
- Library errors become exit codes **here**, never in the core: 0 success, 1 a
  case/skill failed, 2 bad input, 3 provider failure (`skilltest_core::ExitCode`).
  On failure, print the exact problem plus a suggested action on stderr; with
  `--format json`, emit the structured `ReportError` (classified
  `ProviderErrorKind`) so SDKs branch on a typed `kind`, not stderr text.
- `skilltest-fake-provider` is a second `[[bin]]` behind the non-default
  `fake-provider` feature, so a published `cargo install` ships only `skilltest`.
  The nx `build`/`lint` targets and the coverage run enable the feature; release
  builds do not. It implements the `CommandProvider` protocol deterministically —
  only the model is faked — which is why the whole pipeline is testable offline.
  Its fixture conventions are in `tests/AGENTS.md`.
- `tests/oneharness_integration.rs` stays here, `#[ignore]`d and out of the gate:
  the hermetic suite against the real `oneharness` binary (`just
  test-oneharness`, run in CI's `e2e-claude` workflow). Keeping one integration
  test in this package also makes `cargo llvm-cov nextest --workspace` build this
  crate's binaries instrumented, which the CLI e2e suites then drive.
