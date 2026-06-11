# Live e2e against real harnesses

The deterministic gate (`just check`) always fakes the model. This document
covers the **opt-in live e2e**: driving the built `skilltest` CLI against a
*real* harness through `oneharness`, with a real model call, to prove the whole
pipeline end to end. It is never in the gate (money, network, non-determinism),
mirroring the live checks in
[nickderobertis/allowlister](https://github.com/nickderobertis/allowlister).

## Running it

```bash
just install-oneharness          # prebuilt oneharness on PATH (checksum-verified)
just test-live                   # deep claude-code suite (crates/skilltest-cli/tests/live.rs)
just test-harness claude-code    # generic per-harness smoke (scripts/e2e-harness.sh)
just test-harness goose          # any harness id; skips loudly if it can't run here
```

Two layers, both driving the real binary as a subprocess:

- **`test-live`** — the deep, claude-code-specific Rust suite. Asserts the strict
  `pong` reply, multi-turn roles, and normalized usage. This is what CI runs.
- **`test-harness <id>`** — the generic, allowlister-style breadth check. Runs the
  harness-agnostic `tests/fixtures/live/cases/smoke.yaml` and asserts the run
  passed and the reply contained `pong`. The judge is always a **fixed**
  claude-code judge (skilltest's "evals run on a fixed `judge_harness`" model), so
  verdicts stay clean even when the harness under test wraps its reply in a
  banner.

A missing `oneharness`, a missing harness binary, or a missing secret is a
**skip, not a failure** — the rest of the project builds and tests without them.

## Secrets

CI and local runs read harness credentials from the environment. They are managed
declaratively in [`gh-secrets.json`](../gh-secrets.json) and synced from Bitwarden
to both the GitHub repo and a local (gitignored) `.env` with:

```bash
gh-secrets manifest sync
```

Currently synced: `CLAUDE_CODE_OAUTH_TOKEN` (claude-code) and `OPENAI_API_KEY`
(OpenAI-backed harnesses). The fixed claude-code judge means every harness check
also needs `CLAUDE_CODE_OAUTH_TOKEN`.

## Harness matrix

skilltest delivers the skill as a system prompt (`--system`). What a harness can
do therefore depends on whether the installed `oneharness` can carry that system
prompt to it. As of **oneharness v0.2.0**:

| Harness      | Secret (have?)              | Status | Blocker |
| ------------ | --------------------------- | ------ | ------- |
| claude-code  | `CLAUDE_CODE_OAUTH_TOKEN` ✅ | **green** — validated, in CI | — |
| opencode     | `OPENAI_API_KEY` ✅          | blocked | oneharness forwards `--system` as a positional arg; opencode rejects it |
| goose        | `OPENAI_API_KEY` ✅          | blocked | same `--system` forwarding bug (goose rejects it) |
| codex        | `OPENAI_API_KEY` ✅          | blocked | oneharness runs `codex exec -a never`; codex-cli ≥ 0.135 removed `-a` |
| cursor       | `CURSOR_API_KEY` ❌          | needs secret | — |
| crush        | `ANTHROPIC_API_KEY` ❌       | needs secret | — |
| copilot      | `COPILOT_GITHUB_TOKEN` ❌    | needs secret | — |
| qwen         | `OPENAI_API_KEY` + base url ✅/❌ | needs secret + `--system` fix | — |

The "blocked" rows are upstream `oneharness` gaps, not skilltest bugs;
`scripts/e2e-harness.sh` skips them with the exact reason rather than reporting a
false pass. Their config already lives in `scripts/e2e-lib.sh` with
`H_DRIVABLE=0`.

### The two oneharness v0.2.0 findings

1. **`--system` is claude-code-only.** For every harness except claude-code,
   oneharness appends the system text as a positional CLI argument, so the harness
   errors with `unexpected argument '---\nname: …'`. This defeats skilltest's
   skill delivery on those harnesses. Reproduce:
   `oneharness run --harness opencode --system "$(cat tests/fixtures/live/skills/pong/SKILL.md)" --prompt ping`.
2. **codex `-a` flag.** oneharness's codex adapter passes `-a never`, which the
   current `@openai/codex` CLI no longer accepts.

Fixing either unblocks the corresponding rows with no skilltest change. (A
skilltest-side alternative would be an *inline-skill fallback* — prepend the skill
to the prompt when the harness has no system prompt — but that is a provider
change, tracked separately, not part of the e2e harness.)

## Adding a harness

1. **Credential.** Add the harness's secret to Bitwarden, then add it to
   `gh-secrets.json` (`secrets[]`) and run `gh-secrets manifest sync`.
2. **Config.** Confirm `scripts/e2e-lib.sh` has the harness in
   `e2e_harness_config` with the right model / auth env / extra env. When
   oneharness can carry the skill to it, set `H_DRIVABLE=1` and drop `H_BLOCKED`.
3. **Validate locally:** `just test-harness <id>` (it builds the CLI, drives the
   harness, asserts the report).
4. **CI.** Copy `.github/workflows/e2e-claude.yml` to `e2e-<id>.yml`, swap the
   secret name, the install step (`oneharness list` shows each `install_hint`),
   and the run step (`just test-harness <id>`). Keep the `if:` repo+fork gate.
