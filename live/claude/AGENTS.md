# live/claude — the deep claude-code live suite

Nx project `skilltest-live-claude` (`type:live`, `lang:rust`), a `publish = false`
crate holding `tests/live.rs`: the built `skilltest` against **real** oneharness
and a real claude-code, with real model calls (money, network, non-determinism).

- **What it proves.** The `OneharnessProvider` path end to end: `--system` skill
  delivery, `--resume` multi-turn against the real session, boolean + numeric
  judging, the simulated-user loop, and normalized tool events — on the
  near-deterministic fixtures in `tests/fixtures/live/`.
- **Depends on.** `skilltest-cli` (its `live` target builds it first) and
  `skilltest-core`. Nothing may depend on a live project.
- **Run.** `just test-live` (`nx run skilltest-live-claude:live`); needs
  `oneharness` (`just install-oneharness`), the claude CLI and
  `CLAUDE_CODE_OAUTH_TOKEN`. CI runs it from `.github/workflows/e2e-claude.yml`,
  which `ci.yml` calls on every ordinary push to main ahead of the release, never
  on a pull request.

## Rules

- The suite stays `#[ignore]`d — compiled (and linted) by every gate so it
  cannot rot, run only by its `live` target. The `type:live` tag is what keeps
  this project out of both `just check` tiers; never remove it.
- The workflow requires its credential and fails fast without it; never make a
  missing secret skip to green.
