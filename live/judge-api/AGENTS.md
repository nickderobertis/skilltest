# live/judge-api — the direct-API judge live suite

Nx project `skilltest-live-judge-api` (`type:live`, `lang:rust`), a `publish =
false` crate holding `tests/live_api_judge.rs`: the core's `ApiJudgeProvider`
against the **real** Anthropic and OpenAI APIs (no oneharness, no harness CLI).

- **What it proves.** Strict-JSON structured outputs, verdict parsing and usage
  for each vendor, through the real APIs.
- **Depends on.** `skilltest-core` only. Nothing may depend on a live project.
- **Run.** `just test-judge-api` (`nx run skilltest-live-judge-api:live`); needs
  `ANTHROPIC_API_KEY` and/or `OPENAI_API_KEY` (each vendor's test self-skips
  locally without its key). CI runs it from
  `.github/workflows/e2e-judge-api.yml`, which `ci.yml` calls on every ordinary
  push to main ahead of the release; that workflow fails fast unless both keys
  are present, so neither vendor's test can pass by skipping.

## Rules

- The suite stays `#[ignore]`d — compiled (and linted) by every gate, run only by
  its `live` target. Keep the `type:live` tag: it is what keeps this project out
  of both `just check` tiers.
