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
do therefore depends on whether the pinned `oneharness` can carry that to the
model. As of **oneharness v0.2.1** (which delivers `--system` to every harness —
see below):

| Harness      | Secret (have?)              | Status | Note |
| ------------ | --------------------------- | ------ | ---- |
| claude-code  | `CLAUDE_CODE_OAUTH_TOKEN` ✅ | **green** — validated, in CI | native `--append-system-prompt` |
| codex        | `OPENAI_API_KEY` ✅          | **green** — validated, in CI | skill prepended to the prompt |
| goose        | `OPENAI_API_KEY` ✅          | **green** — validated, in CI | native `--system` flag |
| opencode     | `OPENAI_API_KEY` ✅          | blocked | oneharness doesn't extract opencode 1.17.x's `text` event, and the prepended skill (no system flag) can be refused as a policy override |
| cursor       | `CURSOR_API_KEY` ❌          | needs secret | — |
| crush        | `ANTHROPIC_API_KEY` ❌       | needs secret | — |
| copilot      | `COPILOT_GITHUB_TOKEN` ❌    | needs secret | — |
| qwen         | `OPENAI_API_KEY` + base url ✅/❌ | needs secret/config | — |

`scripts/e2e-harness.sh` **skips** a not-yet-green harness with its exact reason
(`H_DRIVABLE=0` + `H_BLOCKED` in `scripts/e2e-lib.sh`) rather than reporting a
false pass.

### How the skill reaches each harness (oneharness v0.2.1)

v0.2.1 ([oneharness#12](https://github.com/nickderobertis/oneharness/pull/12))
fixed two gaps that had limited the live matrix to claude-code:

1. **`--system` reached only claude-code.** Every other adapter dropped the
   system text, so the skill was silently ignored. Now it maps to a native flag
   where one exists (claude-code's `--append-system-prompt`, **goose's
   `--system`**) and is **prepended to the prompt** otherwise (codex, opencode,
   qwen, crush, copilot, cursor), so the instructions always reach the model.
2. **codex `-a never`.** Replaced with `--dangerously-bypass-approvals-and-sandbox`
   (codex-cli ≥ 0.135 removed `-a`).

A third, skilltest-side fix was needed: the provider used to force
`--output-format json` on every harness, which made oneharness try to JSON-extract
the **plain-text** reply of codex/goose and find nothing. It now lets each harness
use its oneharness default format.

**opencode** remains blocked on two things, both downstream of it having no
system-prompt flag: oneharness doesn't yet extract opencode 1.17.x's `text` event,
and a skill prepended as a *user* message can trip opencode's default agent into
refusing it as a "policy override." A true system-prompt path for opencode (and
the matching extraction) would unblock it.

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
