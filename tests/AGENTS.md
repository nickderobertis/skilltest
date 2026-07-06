# tests/ — fixture conventions

`tests/fixtures/` holds the sample skills and test cases the e2e suites share
across all three stacks (Rust, pytest, vitest). Keep them small, deterministic,
and authored for the `skilltest-fake-provider`.

## Layout

- `fixtures/skills/<name>/SKILL.md` — a sample skill. The greeter is the happy
  path; `invalid/` deliberately omits a `description` to exercise validation;
  `deployer/` scripts three shell calls (`git push origin main`, `git status`,
  `rm -rf /tmp/build`) for the mock/spy suites to intercept and observe.
- `fixtures/cases/*.yaml` — sample test cases. Each names the journey it covers
  (`greet_pass`, `greet_fail`, `greet_numeric`, `booking_multiturn`).
- `fixtures/oneharness/` — the hermetic tier: the `fake-claude.sh` shim (a
  scripted claude CLI that executes the installed mock hook) + its `cases/`.
- `fixtures/smoke/` — a self-contained case (`greet.skilltest.yaml`) **and its own
  skill copy**, used by the bundle smoke (`scripts/smoke-{python,npm}-bundle.sh`).
  Kept standalone — no shared skill, no `conftest` above it — so it can be copied into
  a fresh consumer project that has only the published packages installed.

## Authoring fixtures for the fake provider

The fake provider is deterministic, so fixtures encode the expected behaviour
directly (see `docs/protocol.md` for its rules):

- A skill's reply comes from a `fake-reply:` marker in its `SKILL.md` body
  (usually inside an HTML comment). Put the substrings the evals check into that
  reply.
- Each `fake-tool: <name> <command>` marker is one scripted tool call. Under
  `mocks:`, a stub/deny surfaces its output/message into the reply as
  `[<tool>] <text>`, so an eval can assert the canned result reached the model.
- A boolean/numeric `criterion` requires every **backtick-quoted** substring to
  appear in the assistant text. A `turns>=N` token (un-quoted) is true once the
  conversation has N assistant turns — use it for multi-turn `done_when`.
- A simulated user's reply comes from a `say:` marker in its `persona`.

## Keep the failure path real

Every suite must keep at least one fixture that *fails* (`greet_fail.yaml`) and
the malformed/missing-provider paths, so the e2e proves skilltest reports
failures and exits non-zero — not just that the happy path works.

## Live fixtures and the `#[ignore]` exceptions

`tests/fixtures/live/` holds fixtures for the live suite — a `pong` skill that
always replies "pong" and an `echo-ok` two-turn skill — kept *separate* from
`fixtures/cases/` so the deterministic directory-run test still sees exactly the
fake-provider cases. They are near-deterministic on purpose so a real judge has an
unambiguous verdict.

Exactly two suites may be `#[ignore]`, both because they need the `oneharness`
binary on PATH (never to speed up the gate — split genuinely slow journeys into
a target CI still runs instead): `live.rs` (real harness + model; `just
test-live`) and `oneharness_integration.rs` (deterministic, credential-free —
the `fake-claude.sh` shim executes the installed mock hook; `just
test-oneharness`).

When the JSON contract changes, run `just gen-contract` (the SDK models are
generated from the Rust types) and update the fixtures to match.
