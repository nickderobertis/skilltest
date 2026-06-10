# Providers

skilltest never talks to a model directly. A **provider** does three things for
the runner: produce an assistant/skill turn (`respond`), play the simulated user
(`user`), and score the transcript against a criterion (`judge`). There are two
provider backends.

## 1. The oneharness provider (default)

[`oneharness`](https://github.com/nickderobertis/oneharness) (v0.2.0+) is a
prompt→text runner over many agentic harnesses (Claude Code, Codex, OpenCode,
Cursor, …). skilltest's `OneharnessProvider` wires four real oneharness features
into the runner:

- **`--system <text>`** — the skill's instructions are passed as a real system
  prompt (e.g. `--append-system-prompt` for claude-code), not inlined into the
  user turn.
- **`--resume <session>`** — for harnesses that support session continuation
  (`claude-code`, `opencode`, `cursor` today; see `oneharness list` and
  `supports_resume`), the runner threads the `session_id` returned on each turn
  into the next `respond` call, so the harness sees a real continuing
  conversation and keeps its tool state. For harnesses without resume support,
  skilltest falls back to inlining the full transcript on every turn.
- **Normalized `usage`** — `{input_tokens, output_tokens, cost_usd}` is parsed
  off each result and aggregated into the [report](schema.md) so cross-model
  cost reporting is portable instead of harness-specific.
- **Normalized `failure_kind`** — when a run fails with a classified reason
  (`auth`, `rate_limit`, `model_not_found`, `quota`), the CLI maps it to a
  pointed hint instead of a generic provider error.

For each operation skilltest invokes:

```
oneharness run --harness <H> --model <M> --output-format json --compact \
  --timeout <secs> --prompt-file - [--system <skill>] [--resume <session_id>]
```

with a constructed prompt on stdin, then reads `results[0]`: it requires
`status == "ok"` and uses `text`, `session_id`, and `usage`. A non-`ok` status
becomes a provider error (classified by `failure_kind` when set).

| op | harness / model | what skilltest passes |
| --- | --- | --- |
| `respond` | the platform + model under test | `--system <skill instructions>` + either the latest user message (when resuming) or the whole transcript (no-resume harnesses) |
| `user` | `judge_harness` + `judge_model` | the persona, the conversation, and "write only the user's next message" |
| `judge` | `judge_harness` + `judge_model` | the criterion + transcript, then "respond with ONLY `{\"value\": …, \"reason\": …}`" |

Two deliberate choices:

- **Evaluator independence.** `user` and `judge` run on a fixed `judge_harness`
  (default `claude-code`) and `judge_model`, *not* the harness/model under test,
  so the evaluator doesn't vary across the matrix.
- **Tolerant verdict parsing.** Real models don't always emit bare JSON, so the
  judge response is extracted from the first `{…}` in the text (code fences and
  surrounding prose are tolerated) and then type-checked against the eval kind.

Configure it (see `docs/schema.md`):

```yaml
provider:
  kind: oneharness
  bin: oneharness          # resolved on PATH
  judge_harness: claude-code
  timeout_secs: 120
```

`platforms` are oneharness harness ids; `models` must be valid for the chosen
harness (e.g. `sonnet`, `haiku`, or a full model id for `claude-code`).

## 2. The custom command protocol (JSON-lines)

The second backend speaks a small JSON-lines protocol and backs both the bundled
deterministic `skilltest-fake-provider` (which is how the gate runs without a
model) and any provider you write. skilltest spawns the command once per op,
writes **one** JSON request object to stdin (newline-terminated), and reads
**one** JSON response object from stdout; a non-zero exit (message on stderr) is a
provider error.

```yaml
provider:
  kind: command
  command: ["skilltest-fake-provider"]   # or ["python3", "my_provider.py"]
```

Every request has an `op` and a `messages` array (`{role, content}`).

**`respond`** — request carries `platform`, `model`, `skill` (`{name, path,
instructions}`), `messages`, and an optional `session` (a handle the runner
captured from a prior `respond` so a stateful provider can continue);
response: `{"message": "...", "done": false}`, plus optional `usage`
(`{input_tokens, output_tokens, cost_usd}`, all individually optional) and
`session_id` (which the runner will pass back as `session` next turn).

**`user`** — request carries `model`, `persona`, `messages`; response:
`{"message": "...", "stop": false}`, plus optional `usage`.

**`judge`** — request carries `model`, `kind` (`"boolean"`/`"numeric"`),
`criterion`, `messages`, plus `min`/`max` for numeric; response:
`{"value": <bool|number>, "reason": "..."}`, plus optional `usage`. `value`
must be a boolean for boolean evals and a number for numeric; a mismatch is a
provider error.

`usage` and `session_id` are entirely optional — a stateless provider (like
the bundled fake) simply omits them and the report's usage totals stay empty.

A reference implementation is
[`crates/skilltest-cli/src/bin/fake_provider.rs`](../crates/skilltest-cli/src/bin/fake_provider.rs).
