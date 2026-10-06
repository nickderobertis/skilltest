# crates/skilltest-core — the library

Nx project `skilltest-core` (`type:lib`, `lang:rust`). The reusable Rust API the
CLI builds on: config, the skill model + validation, the test-case model, the
provider protocol and its backends, evals, tool mocking, the runner, the report —
and the Rust types that are the single source of truth for the JSON contract
(`schemas/` is generated from them).

- **What it proves.** `pnpm exec nx run skilltest-core:test` runs the unit
  suites inside `src/`: the conversation loop, eval scoring, mock
  compilation/matching, config and case parsing, provider response parsing —
  against in-process fakes. The CLI's binary e2e and the whole-workspace
  coverage floor prove it end to end.
- **Depends on.** Nothing in this repo. Everything else depends on it, directly
  or through the CLI, so a change here selects the CLI, its e2e project, both
  SDKs and both framework packages.

## Rules for this crate

- Errors use `thiserror`; this crate never maps an error to a process exit code
  (that boundary is the CLI's). The exit-code *values* live in `src/exit.rs`.
- Every external input is parsed into a typed model before use — config, case
  YAML/JSON, skill frontmatter, every provider response. Case input is strict
  everywhere: unknown fields are rejected even inside evals, the `user` block and
  the map forms of `stub`/`deny`/field predicates (the `Eval` enum wraps
  `deny_unknown_fields` structs in newtype variants because serde cannot deny on
  an internally tagged enum). A typo'd key must never silently apply a default.
- Changing a type that reaches `--format json` or the case input changes the
  contract: run `just gen-contract` and commit `schemas/` + the generated SDK
  models in the same change.

## The provider boundary

<!-- llmlint: ignore-block[agents_md_durable_and_terse] Moved here from the root AGENTS.md with this same reason: it is the summary of which oneharness features skilltest depends on and why — the maintenance constraints behind each — while docs/protocol.md holds the wire detail. -->

`skilltest` never talks to a model directly. The `Provider` trait
(`src/provider.rs`) has two real backends:

- **`OneharnessProvider` (default).** Targets
  [`oneharness`](https://github.com/nickderobertis/oneharness) **v0.16.0** (the
  line both SDKs bundle and `scripts/install-oneharness.sh` installs;
  `crates/skilltest-cli-e2e/tests/pins.rs` reconciles every restatement). It uses
  oneharness's normalized features rather than string-munging: `--system <skill
  instructions>` carries the skill as a real system prompt; `--resume
  <session_id>` continues a real harness session for the multi-turn loop where
  `supports_resume` is true (others inline the transcript); `--events` surfaces
  normalized tool events (`{kind, name, input, output, index}`) lifted onto each
  assistant turn (`Message.events`); `results[*].usage` aggregates into the
  report; `results[*].failure_kind` (`auth`/`rate_limit`/`model_not_found`/
  `quota`) becomes `Error::Provider { kind }` so the CLI prints a pointed hint;
  and `--history --history-dir <dir> --history-name <name>` records each
  **skill** run to a shared history directory (default `<state
  dir>/skilltest/oneharness-history`; `provider.history_dir` /
  `SKILLTEST_HISTORY_DIR` override it, `provider.history: false` disables it),
  whose echoed `history_file` becomes `CaseRun.history_command`. Judge and
  simulated-user calls are never recorded.
- skilltest passes **no `--mode`**, so oneharness's own default approval mode
  applies; users set `bypass` etc. through oneharness config (`ONEHARNESS_MODE`),
  keeping approval policy in one place.
- **Omit `--model`** when it is unspecified so the harness uses its own default,
  and fall back to a harness's **raw stdout** when oneharness reports no text
  (defense in depth for its "text may be null" contract).
- A streaming variant (`respond_streaming`, `oneharness run --stream`) forwards
  tool events live and, on a sink `ControlFlow::Break`, kills the oneharness
  child to short-circuit a bad run; the buffered `respond` (`--compact`) is the
  default. Evals and the simulated user run on a fixed `judge_harness`,
  independent of the harness under test. Verdict JSON is parsed tolerantly (real
  models wrap it in prose/fences) and type-checked.
- **`CommandProvider`.** A small JSON-lines protocol (one request object on
  stdin, one response on stdout, per op) — the bundled `skilltest-fake-provider`
  and any custom provider. Custom providers may emit `usage`, `session_id` and
  `events` on `respond`, and may honor the `mocks` request block (returning
  `mock_calls`); ignoring the block while it is present is a loud provider error.
  The protocol is a cross-repo contract: never change it unilaterally.

**Tool mocking/spying.** `src/mock.rs` compiles a case's `mocks:` (and the CLI's
`--mocks`/`--spy`, the SDKs' delivery path) to the oneharness ruleset;
`OneharnessProvider` passes `run --mock-rules`/`--spy-file` per skill turn (never
to the judge) and parses the spy JSONL into `CaseRun.mock_calls` (original
pre-rewrite inputs + verdicts). There are two matching engines on purpose — the
hook-side `oneharness mock`, mirrored by `mock::decide` for the fake provider —
and `just test-oneharness` plus the live e2e are their drift alarms. Anything
inexpressible or unresolvable errors loudly, never a vacuous pass.

<!-- llmlint: ignore-end[agents_md_durable_and_terse] -->
