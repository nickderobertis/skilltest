# tests/ — fixture conventions

`tests/fixtures/` holds the sample skills and test cases the e2e suites share
across all three stacks (Rust, pytest, vitest). Keep them small, deterministic,
and authored for the `skilltest-fake-provider`.

## Layout

- `fixtures/skills/<name>/SKILL.md` — a sample skill. The greeter is the happy
  path; `invalid/` deliberately omits a `description` to exercise validation.
- `fixtures/cases/*.yaml` — sample test cases. Each names the journey it covers
  (`greet_pass`, `greet_fail`, `greet_numeric`, `booking_multiturn`).

## Authoring fixtures for the fake provider

The fake provider is deterministic, so fixtures encode the expected behaviour
directly (see `docs/protocol.md` for its rules):

- A skill's reply comes from a `fake-reply:` marker in its `SKILL.md` body
  (usually inside an HTML comment). Put the substrings the evals check into that
  reply.
- A boolean/numeric `criterion` requires every **backtick-quoted** substring to
  appear in the assistant text. A `turns>=N` token (un-quoted) is true once the
  conversation has N assistant turns — use it for multi-turn `done_when`.
- A simulated user's reply comes from a `say:` marker in its `persona`.

## Keep the failure path real

Every suite must keep at least one fixture that *fails* (`greet_fail.yaml`) and
the malformed/missing-provider paths, so the e2e proves skilltest reports
failures and exits non-zero — not just that the happy path works.

## The one `#[ignore]` exception

`crates/skilltest-cli/tests/live.rs` is the *only* test allowed to be
`#[ignore]`. It needs a real provider and credentials, so it is opt-in
(`SKILLTEST_LIVE_PROVIDER=... cargo test --test live -- --ignored`) and stays out
of the deterministic gate. Do **not** add `#[ignore]` to any other e2e test to
speed up the gate — split genuinely slow journeys into a target CI still runs
instead.

When the JSON contract changes, update fixtures and the three stacks' parsers
(serde / Pydantic / Zod) together.
