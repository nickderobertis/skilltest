# Config and test-case schema

## Config file (`skilltest.yaml`)

Loaded from `skilltest.yaml` in the working directory by default, or from
`--config <path>`. Every field has a default, and CLI flags
(`--provider`, `--platform`, `--model`, `--judge-model`, `--max-turns`) override
the file.

```yaml
# The provider (see docs/protocol.md). The default runs skills through
# oneharness; `kind: command` runs a custom JSON-lines provider instead.
provider:
  kind: oneharness          # or: command
  bin: oneharness           # oneharness binary, resolved on PATH
  judge_harness: claude-code # harness used for evals + the simulated user
  timeout_secs: 120         # passed to `oneharness run --timeout`
  # For kind: command instead:
  #   kind: command
  #   command: ["skilltest-fake-provider"]

# Harness platforms a case runs on; a run fans out over platforms × models.
# Platforms are oneharness harness ids.
platforms: ["claude-code"]

# Models a case runs on (must be valid for the chosen harness).
models: ["sonnet"]

# Model used for natural-language evals and the simulated user.
# Defaults to the first entry of `models` when omitted.
judge_model: "haiku"

# Default cap on assistant turns for multi-turn cases (a case may lower it).
max_turns: 8
```

CLI flags override the file: `--provider "<argv>"` switches to a command
provider; `--oneharness-bin`, `--judge-harness`, and `--timeout` tune the
oneharness provider; `--platform`, `--model`, `--judge-model`, and `--max-turns`
override the rest.

## Test case (`*.yaml`, or `*.skilltest.yaml` for plugin auto-collection)

```yaml
# Optional; defaults to the file stem.
name: greet_pass

# Path to the skill directory under test (a dir containing SKILL.md),
# resolved relative to this file.
skill: ../skills/greeter

# The initial data/prompt handed to the skill as the first user message.
input: "Greet Dr. Smith, who has an appointment today."

# Optional. Present => multi-turn: after each assistant turn a simulated user
# replies until `done_when` holds, the skill reports done, or max_turns is hit.
user:
  persona: "You are a terse patient confirming an appointment."
  done_when: "the assistant has confirmed a booking"   # optional
  max_turns: 5                                          # optional override

# The evals that decide pass/fail. Must be non-empty; all must pass.
evals:
  - type: boolean
    name: names-the-patient        # optional label for reports
    criterion: "the reply greets Dr. Smith by name"
    expected: true                 # optional, default true

  - type: numeric
    criterion: "how completely was the appointment confirmed"
    min: 0
    max: 10
    threshold: 7
    comparator: ">="               # one of >= > <= < (default >=)
```

A **single-turn** case omits `user`: the skill produces one assistant turn, then
the evals score it. A **multi-turn** case includes `user` and loops.

### Eval pass rules

- **boolean** passes when the judge's verdict equals `expected` (default `true`).
- **numeric** clamps the judge's score to `[min, max]`, then passes when it
  satisfies `comparator` against `threshold`.

A case run passes when every eval passes. A `skilltest run` exits `0` when all
runs pass and `1` when any fail.

## Report (`--format json`)

The stable JSON contract the plugins parse (Pydantic in pytest, Zod in vitest).
Each run and the top-level summary may carry a `usage` object aggregated from
every provider call:

```json
{
  "passed": true,
  "summary": {
    "cases": 1, "runs": 1, "passed": 1, "failed": 0,
    "usage": {"input_tokens": 5616, "output_tokens": 46, "cost_usd": 0.0124}
  },
  "runs": [{
    "case": "pong", "skill": "…/skills/pong",
    "platform": "claude-code", "model": "haiku",
    "passed": true, "turns": 1,
    "evals": [{"label": "…", "passed": true, "detail": {…}, "reason": "…"}],
    "transcript": {"messages": […]},
    "usage": {"input_tokens": 5616, "output_tokens": 46, "cost_usd": 0.0124}
  }]
}
```

`usage.input_tokens`, `usage.output_tokens`, and `usage.cost_usd` are each
independently optional — `null` / absent means "this harness did not report
the signal," not zero. The whole `usage` object is omitted when nothing
reported usage (e.g. the fake provider in the gate). Cost is commonly absent
on subscription auth.
