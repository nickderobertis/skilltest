# schemas/ — the CLI ↔ SDK contract

Nx project `skilltest-contract` (`type:contract`, `lang:json`). The golden JSON
Schemas (draft-07) generated from the Rust types in `skilltest-core`, and the
source every SDK's models are generated from:

- the **output contract** — `report`/`validation` (the `--format json` shapes)
  and `error` (the structured `ReportError` a failure exit emits, carrying a
  classified `ProviderErrorKind`);
- the **input contract** — `case`, the test-case shape (`--case-json`/YAML),
  additionally pinned by the kitchen-sink golden
  `tests/fixtures/contract/case_kitchen_sink.json`, which the Rust construction
  and both SDKs' case builders must each serialize to exactly.

- **What it proves.** Its `contract-check` target (`scripts/gen-contract.sh
  --check`) regenerates every artifact into a staging dir and fails on any drift
  from what is committed — schemas and the generated SDK models alike. It is
  uncached, and every `just check` tier runs it, because the contract spans
  every stack.
- **Depends on.** Nothing (a `type:contract` project may depend only on other
  contracts). Its inputs are this directory, `scripts/gen-contract.sh` and
  `tests/fixtures/contract/`, so a change to any of them selects it and its
  dependents — both SDKs and `skilltest-cli-e2e` — and not every project.
- **Run.** After a contract type changes, regenerate with `just gen-contract`
  and commit everything it rewrites; every `just check` tier verifies.

## Rules

- Never hand-edit a file here or a generated SDK model
  (`sdks/python/skilltest_sdk/_*.py`, `sdks/typescript/src/generated/*`). Change
  the Rust types, run `just gen-contract`, commit everything it rewrites.
- The shape is a **stable contract** the SDKs and their users depend on. A
  breaking change lands behind a `feat!:`/`BREAKING CHANGE` commit so the
  lockstep version moves (never hand-bumped); prefer additive optional fields,
  omitted when empty, so old consumers are unaffected.
- A new SDK language adds its generator invocation and output paths to
  `scripts/gen-contract.sh` (the language's standard JSON-Schema-to-types
  generator, quicktype as the fallback).
