# Providers

skilltest never talks to a model directly. A **provider** does three things for
the runner: produce an assistant/skill turn (`respond`), play the simulated user
(`user`), and score the transcript against a criterion (`judge`). There are two
provider backends.

## 1. The oneharness provider (default)

[`oneharness`](https://github.com/nickderobertis/oneharness) is a stateless
prompt→text runner: `oneharness run --harness H --model M --prompt-file -` runs a
prompt on a harness (Claude Code, Codex, …) and returns one JSON document whose
`results[0].text` is the harness's final message. It has no skill, judge, user,
or session concept — so skilltest's provider **builds the prompts** and parses
the result.

For each operation skilltest invokes:

```
oneharness run --harness <H> --model <M> --output-format json --compact \
  --timeout <secs> --prompt-file -
```

with a constructed prompt on stdin, then reads `results[0]`: it requires
`status == "ok"` and uses `text` (a non-`ok` status or missing text is a provider
error).

| op | harness / model | prompt skilltest builds |
| --- | --- | --- |
| `respond` | the platform + model under test | the skill instructions inlined, then the conversation, then "write only the assistant's next reply" |
| `user` | `judge_harness` + `judge_model` | the persona, then the conversation, then "write only the user's next message" |
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
instructions}`), `messages`; response: `{"message": "...", "done": false}`
(`done` optional).

**`user`** — request carries `model`, `persona`, `messages`; response:
`{"message": "...", "stop": false}` (`stop` optional).

**`judge`** — request carries `model`, `kind` (`"boolean"`/`"numeric"`),
`criterion`, `messages`, plus `min`/`max` for numeric; response:
`{"value": <bool|number>, "reason": "..."}`. `value` must be a boolean for
boolean evals and a number for numeric; a mismatch is a provider error.

A reference implementation is
[`crates/skilltest-cli/src/bin/fake_provider.rs`](../crates/skilltest-cli/src/bin/fake_provider.rs).
