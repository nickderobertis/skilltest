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

Currently synced: `CLAUDE_CODE_OAUTH_TOKEN` (claude-code, and the fixed judge),
`OPENAI_API_KEY` (codex, goose, qwen), `ANTHROPIC_API_KEY` (opencode, crush),
`CURSOR_API_KEY` (cursor), and `COPILOT_GITHUB_TOKEN` (copilot, sourced from the
Bitwarden `GH_TOKEN` item). The fixed claude-code judge means **every** harness
check also needs `CLAUDE_CODE_OAUTH_TOKEN`, regardless of the harness under test.

## Harness matrix

skilltest delivers the skill as a system prompt (`--system`). What a harness can
do therefore depends on whether the pinned `oneharness` can carry that to the
model and whether we hold a credential. As of **oneharness v0.2.5**, the entire
matrix is **green** — every harness skilltest knows about is validated and runs
in CI:

| Harness      | Secret (have?)              | Model (e2e default)            | Status | Skill delivery / reply extraction |
| ------------ | --------------------------- | ------------------------------ | ------ | --------------------------------- |
| claude-code  | `CLAUDE_CODE_OAUTH_TOKEN` ✅ | `haiku`                        | **green** — in CI | native `--append-system-prompt`; json `result` |
| codex        | `OPENAI_API_KEY` ✅          | `gpt-5-mini`                   | **green** — in CI | prepended to prompt; raw text |
| goose        | `OPENAI_API_KEY` ✅          | env `GOOSE_MODEL=gpt-5-mini`   | **green** — in CI | native `--system`; raw text |
| opencode     | `ANTHROPIC_API_KEY` ✅       | `anthropic/claude-haiku-4-5`   | **green** — in CI | prepended to prompt; **raw-stdout fallback** (JSONL) |
| cursor       | `CURSOR_API_KEY` ✅          | CLI default                    | **green** — in CI | prepended to prompt; stream-json `result` |
| crush        | `ANTHROPIC_API_KEY` ✅       | CLI default (Anthropic)        | **green** — in CI | prepended to prompt; raw text |
| qwen         | `OPENAI_API_KEY` ✅          | `gpt-4o-mini` (OpenAI-compat)  | **green** — in CI | prepended to prompt; raw text |
| copilot      | `COPILOT_GITHUB_TOKEN` ✅    | CLI default                    | **green** — in CI | prepended to prompt; raw text |

`scripts/e2e-harness.sh` still **skips** (never falsely passes) when a harness
binary, `oneharness`, or a secret is missing — and a future not-yet-drivable
harness should be added with `H_DRIVABLE=0` + a precise `H_BLOCKED` reason in
`scripts/e2e-lib.sh`.

### How the skill reaches each harness, and how the reply comes back

Two layers had to line up to make the whole matrix green:

1. **`--system` delivery (oneharness v0.2.1,
   [#12](https://github.com/nickderobertis/oneharness/pull/12)).** It maps to a
   native flag where one exists (claude-code's `--append-system-prompt`, goose's
   `--system`) and is **prepended to the prompt** otherwise (codex, opencode,
   cursor, crush, qwen, copilot), so the skill always reaches the model. The same
   release replaced codex's removed `-a never` with
   `--dangerously-bypass-approvals-and-sandbox` (codex-cli ≥ 0.135).

2. **Reply extraction + model selection (skilltest-side).** Three provider fixes:
   - The provider no longer forces `--output-format json`; each harness uses its
     oneharness default format (forcing json made oneharness json-extract the
     plain-text reply of codex/goose and find nothing).
   - **Raw-stdout fallback.** oneharness extracts the reply into `text` on a
     best-effort basis and leaves it null when a harness's output shape defeats
     extraction — OpenCode emits JSONL with the reply nested in a `part`. skilltest
     now falls back to the raw stdout (where the reply still lives) instead of
     erroring, matching oneharness's documented contract. This is what makes
     opencode scorable without an upstream extraction change.
   - **Empty model means "use the harness default."** cursor/crush/copilot pick a
     sensible default and qwen reads `OPENAI_MODEL`, so skilltest omits `--model`
     when it is unspecified rather than forwarding a broken empty flag.

   One harness-specific gotcha: **qwen** speaks an OpenAI-compatible API but its
   client sends `max_tokens`, which the **gpt-5 family rejects** (they require
   `max_completion_tokens`). The e2e points qwen at `gpt-4o-mini`; gpt-5-mini 400s.

The only rough edge left is cosmetic: opencode's transcript is its raw JSONL
rather than a clean extracted message. A proper opencode text-extraction in
oneharness (it already lifts opencode's usage/session signals) would tidy that
up; the smoke passes today because the judge and the assertion both find the reply
inside the JSONL.

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
