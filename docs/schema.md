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

# Optional. Mock/spy declarations: an entry WITH one of `stub`/`deny`/`rewrite`
# is a mock (matching tool calls are intercepted inside the harness); one
# without any action is a spy (observed only). First matching mock wins; a call
# no mock matches runs normally and is still recorded. `called`/`not_called`
# evals reference entries by `name`.
mocks:
  - name: push
    # Every given criterion must hold. `tool` is the (per-harness) tool name,
    # case-insensitive; `contains` a substring of the raw hook-event JSON;
    # `pattern` an unanchored linear-time regex over the same haystack (no
    # lookarounds; the haystack is JSON, so quotes in tool input are escaped);
    # `input` gives per-argument predicates (`equals`/`contains`/`pattern`,
    # or a bare string for equality) — absent fields fail the match.
    match: { tool: bash, pattern: "git push( --force)?\b" }
    # Fake a SHELL call's result by declaring only the output: nothing real
    # runs, and the model receives this text as the tool's genuine result.
    # Also `stub: { output: ..., exit_code: 2 }` to fake a failing command.
    stub: Everything up-to-date
  - name: danger
    match: { contains: "rm -rf" }
    # Block the call; the model reads the message as the tool's feedback.
    deny: destructive commands are blocked
  - name: config
    match: { tool: read, input: { file_path: { contains: "config.prod" } } }
    # The low-level rewrite: substitute raw input fields (here redirecting a
    # file read to a fixture). Shell stubs are better written with `stub`.
    rewrite: { file_path: fixtures/config.yaml }
  - name: git                       # no action => a spy
    match: { tool: bash, pattern: "\bgit\b" }

# Optional (default false). Record every tool call through the mock/spy channel
# even with no `mocks` — the report then carries `mock_calls` for code-level
# spies. Implied whenever `mocks` is non-empty.
spy: false

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

  # Deterministic (no judge): assert on the mock/spy channel's observed calls,
  # referencing a `mocks` entry by name.
  - type: called
    mock: push
    times: 1                       # optional exact count; absent = at least 1
    where: { command: { contains: "origin" } }   # optional input predicates

  - type: not_called
    mock: danger
```

A **single-turn** case omits `user`: the skill produces one assistant turn, then
the evals score it. A **multi-turn** case includes `user` and loops.

### Defining a case in code (the recommended form)

The YAML above is one way to write a case; the SDKs let you build the same case
in code and run it directly — the recommended approach, since the case, its
mocks, and any deterministic transcript checks stay in one typed place. The
builders map one-to-one onto the fields above:

```python
# Python (skilltest-sdk / skilltest-pytest)
from skilltest_sdk import TestCase, run_skill, boolean, numeric, called, stub, user

case = TestCase(
    skill="skills/greeter",          # relative to the working directory, not a file
    input="Greet Dr. Smith, who has an appointment today.",
    user=user("a terse patient", done_when="the appointment is confirmed"),  # optional
    mocks=[stub(pattern=r"git push\b", output="Everything up-to-date", name="push")],
    evals=[
        boolean("the reply greets Dr. Smith by name"),
        numeric("how warm is the tone", min=0, max=10, threshold=7),
        called("push", times=1),     # references the named mock above
    ],
)
report = run_skill(case)
```

```ts
// TypeScript (@skill-test/sdk / @skill-test/vitest)
import { runSkill, testCase, boolean, numeric, called, stub, user } from "@skill-test/sdk";

const report = await runSkill(testCase({
  skill: "skills/greeter",
  input: "Greet Dr. Smith, who has an appointment today.",
  user: user("a terse patient", { doneWhen: "the appointment is confirmed" }),
  mocks: [stub({ pattern: /git push\b/, output: "Everything up-to-date", name: "push" })],
  evals: [
    boolean("the reply greets Dr. Smith by name"),
    numeric("how warm is the tone", { min: 0, max: 10, threshold: 7 }),
    called("push", { times: 1 }),
  ],
}));
```

Under the hood the SDK serializes the case to JSON and runs `skilltest run
--case-json <file>`; unlike a YAML `PATH`, a code-defined case's `skill`
resolves relative to the **working directory** (the SDKs run the CLI from your
project). The CLI validates the compiled case exactly as it validates YAML, so a
malformed case is a loud usage error, never a vacuous pass. YAML files remain
first-class — a path works everywhere a code-defined case does, and the plugins
still auto-discover `*.skilltest.yaml` files.

Mocking/spying is delivered per run with zero permanent config mutation
(oneharness's `run --mock-rules`/`--spy-file`); what a harness supports varies
(rewrite/stub work on claude-code, codex, opencode, crush, cursor; goose is
deny-only; qwen and copilot cannot take the per-run delivery at all — qwen's
hooks fire only at user scope headlessly, copilot's never do) and anything
inexpressible is a **loud usage error**, never a silent allow. Mocks apply only to the harness
under test — never to the judge or the simulated user.

### Eval pass rules

- **boolean** passes when the judge's verdict equals `expected` (default `true`).
- **numeric** clamps the judge's score to `[min, max]`, then passes when it
  satisfies `comparator` against `threshold`.
- **called** passes when the referenced mock/spy observed at least one (or
  exactly `times`) matching call(s); **not_called** when it observed none.
  Both are scored deterministically from the observed records — no judge —
  and error loudly when the run has no observation channel rather than passing
  vacuously. A mock's calls are the ones *its rule intercepted*; a spy's are
  everything its matcher covers (including calls other mocks intercepted).

A case run passes when every eval passes. A `skilltest run` exits `0` when all
runs pass and `1` when any fail.

## Report (`--format json`)

The stable JSON contract the language SDKs parse (Pydantic in `skilltest-sdk`,
Zod in `@skill-test/sdk`). Each run and the top-level summary may carry a
`usage` object aggregated from every provider call:

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
    "transcript": {"messages": [
      {"role": "user", "content": "…"},
      {"role": "assistant", "content": "…", "events": [
        {"kind": "tool_call", "name": "bash",
         "input": {"command": "git commit -m x"}, "index": 0}
      ]}
    ]},
    "usage": {"input_tokens": 5616, "output_tokens": 46, "cost_usd": 0.0124}
  }]
}
```

`usage.input_tokens`, `usage.output_tokens`, and `usage.cost_usd` are each
independently optional — `null` / absent means "this harness did not report
the signal," not zero. The whole `usage` object is omitted when nothing
reported usage (e.g. the fake provider in the gate). Cost is commonly absent
on subscription auth.

Each assistant `Message` carries an **`events`** array: the normalized tool
events the skill took producing that turn (`kind` is `tool_call` or
`tool_result`; `name` is the normalized tool where knowable; `input` is the
structured, tool-shaped args; `output` is the observation when exposed; `index`
is the position in the turn). It is lifted from oneharness's `--events` output,
so consumers can assert on *what the skill did* — shell commands, file edits,
tool uses — not just its final text. The array is empty (omitted) for harnesses
that expose no machine-readable transcript. The same events are also streamed
live by the SDKs' streaming API, for short-circuiting a bad run.

When the mock/spy channel is on (`mocks`/`spy` in the case, `--mocks`/`--spy`
on the CLI), each run also carries **`mock_calls`**: every observed tool call
in order, as `{tool, input, action, rule, mock}` — `input` is the **original,
pre-rewrite** arguments (the transcript's `events` show post-rewrite reality,
e.g. the printf a stub compiled to), `action` is the verdict applied
(`allow`/`deny`/`rewrite`/`stub`), and `mock` names the declaration whose rule
intercepted. `mock_calls` is `null` when the channel was off and `[]` when it
was on with no tool calls — the SDKs rely on that distinction so an unbound
spy errs instead of reading as "zero calls". Deterministic eval outcomes use
the `calls` detail kind: `{kind: "calls", count, times, negated}`.

## Output contract: how the CLI and the SDKs stay in sync

The Rust report types (`crates/skilltest-core/src/report.rs` and friends) are
the single source of truth for the JSON contract, and every SDK's models are
**generated** from them — no per-language model code is written or reviewed by
hand. The chain, all driven by `scripts/gen-contract.sh` (`just gen-contract`):

1. The types derive `schemars::JsonSchema`, and `skilltest schema
   <report|validation>` emits their JSON Schema (draft-07 on purpose — the
   dialect the generators below digest reliably).
2. The script writes those schemas to `schemas/report.schema.json` and
   `schemas/validation.schema.json` (the **goldens**), then generates each
   SDK's models from them:
   - Python: [`datamodel-code-generator`](https://github.com/koxudaxi/datamodel-code-generator)
     → Pydantic v2 models in `skilltest_sdk/_report.py` / `_validation.py`, so
     Python keeps full runtime validation for free.
   - TypeScript: [`json-schema-to-typescript`](https://www.npmjs.com/package/json-schema-to-typescript)
     → type declarations in `src/generated/` (types only by design; the drift
     gate is what guarantees the shape, so the runner casts after `JSON.parse`
     instead of re-validating).
3. **Drift gate**: `just contract-check` (part of `just check`) regenerates
   everything into a staging dir and diffs it against the checked-in
   artifacts, so a contract change that skips regeneration — or a hand-edit of
   generated code — fails CI with the exact diff. A Rust e2e test additionally
   pins the binary to the checked-in goldens.

Hand-written code never restates the contract's shape: helpers like
`describe_failures`/`assistantText` live in thin facades (`models.py`,
`helpers.ts`) typed against the generated models, so `ty`/`tsc` catch any
helper that mentions a field that no longer exists.

To change the contract: change the Rust types, run `just gen-contract`, commit
the regenerated artifacts, and bump versions (a shape change is breaking for
SDK consumers). Generated files are excluded from lint/format style rules (they
are not hand-maintained), but type checkers still cover them.

To add a language: pick the language's standard JSON-Schema-to-types generator
(quicktype as the fallback), add its invocation and output paths to
`scripts/gen-contract.sh`, and pin its version in that SDK's lockfile so the
drift gate is deterministic.

At runtime the SDKs stay tolerant on purpose: unknown JSON keys are ignored so
an older SDK can read a newer CLI's output, while required fields are still
enforced where validation exists.
