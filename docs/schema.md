# Config and test-case schema

## Config file (`skilltest.yaml`)

The Rust `Config` type is the authority for every setting. `skilltest schema
config` emits its draft-07 schema, checked in as `schemas/config.schema.json`.
Both SDKs export generated `Config` and recursive `ConfigOverrides` types.
The core resolver is shared by configuration loading and CLI runs; SDKs only
serialize overrides into the CLI's repeatable `--set` transport.

| Priority (low → high) | Surface |
| --- | --- |
| 1 | Built-ins, including existing binary/history environment defaults |
| 2 | YAML/JSON file: `skilltest.yaml`, or `--config FILE` |
| 3 | Derived `SKILLTEST_` configuration environment variables |
| 4 | `run --set dotted.path=<JSON-value>`, in command order |
| 5 | Existing named CLI flags and SDK shorthand options; explicit mocks prepend and spy enables |

Objects merge recursively. Arrays replace as whole values, including role args,
mocks, models and platforms; there is no array-index path syntax. Explicit
`false`, `0`, empty arrays and `null` are values, subject to the setting's
validation. `null` clears only nullable strings/blocks; it does not clear a
required number, boolean, string or array. Changing `provider.kind` or
`judge.kind` discards the previous variant's fields before merging the new
variant. Unknown properties, wrong types and invalid combinations fail.

`--set` splits on the first `=` and requires a JSON value, preserving the
string and argv token contents. For example:

```sh
skilltest run cases/ --set 'provider.history=false' \
  --set 'provider.judge.args=["--harness","goose","--env","ROLE=judge with spaces"]'
```

Environment names follow property paths: uppercase each segment and join with
`__` after `SKILLTEST_`, such as `SKILLTEST_PROVIDER__JUDGE__CONFIG` and
`SKILLTEST_PROVIDER__JUDGE__ARGS`. String/enum leaves accept bare text or
JSON-quoted strings; everything else requires JSON. Unquoted `null` clears a
nullable setting, while `"null"` is literal string text. Nullable blocks are
clearable at their own path (`SKILLTEST_JUDGE=null`); arrays require a whole
JSON array. The existing operational `SKILLTEST_BIN`, `SKILLTEST_PROVIDER`,
`SKILLTEST_ONEHARNESS_BIN` and `SKILLTEST_HISTORY_DIR` retain their meanings.
SDK `bin`/`cwd` control process placement and are outside the run-config schema.

```yaml
provider:
  kind: oneharness
  bin: oneharness
  judge_harness: claude-code
  timeout_secs: 120
  history: true
  history_dir: null
  # Full invocation configuration: omitted settings belong to oneharness.
  judge:
    config: judge.oneharness.toml
    args: ["--schema-max-retries", "1", "--env", "ROLE=judge"]
  simulated_user:
    config: user.oneharness.toml
    args: ["--harness", "goose"]
platforms: ["claude-code"]
models: ["sonnet"]
judge_model: "haiku"
max_turns: 8
mocks: []
spy: false
# Independent API routing remains available: judge: {kind: api, vendor: openai}.
judge: null
```

`provider.judge` is separate from the top-level API `judge` block. An absent
`provider.simulated_user` inherits `provider.judge`; an explicit `{}` opts into
oneharness defaults independently. With both role blocks absent, the legacy
`judge_harness`, effective `judge_model` (falling back to the first `models`
entry) and `timeout_secs` behavior is preserved. Role config paths, cwd settings
and raw argv resolve from the invocation working directory, not the YAML file.

Evaluator shorthands are layer-aware. A higher-layer `judge_harness`,
`judge_model` or provider `timeout_secs` overrides that corresponding setting
of a lower-layer role block. A same/higher-layer role block owns its omitted
settings; built-in/fallback model values are never explicit overrides.
For example, the role blocks in the YAML above own their settings despite the
same-file `judge_model: haiku`. `SKILLTEST_JUDGE_MODEL=sonnet` overrides their
model; an environment role block would own same-layer omitted defaults.
Within `--set`, a later role-block assignment owns defaults over an earlier
shorthand, and a later shorthand overrides an earlier role assignment. Named
`--judge-harness`, `--judge-model` and `--timeout` always win for both role
calls, removing only matching role option tokens (including `--flag=value`).
Tokens after `--` remain literal harness argv. oneharness owns the precedence
between its CLI args and its config file.

Role invocations must produce a single buffered verdict. Compatible settings
include oneharness config paths, schema retry count, timeouts, cwd, env, approval
mode, harness/model selection, fallback chains and harness-tail argv. Parallel
fan-out, batch/prompt replacement, streaming, report-format overrides and an
explicit role-args `--schema` are incompatible. Multiple successful results
fail rather than silently becoming one verdict. The judge receives a temporary
schema through oneharness's native/prompt delivery; it overrides a role config's
schema. Responder and simulated-user calls receive no skilltest-generated
schema; their user-owned oneharness schema settings remain oneharness's concern.
Schema validation/retries belong to oneharness, so a retry setting of N can cost
up to 1 + N model invocations. A validated structured verdict takes priority;
otherwise skilltest uses the last complete correctly typed verdict in text,
ignoring trailing prose or an incomplete reconsideration.

Python `run_skill`/`stream_skill` accept `config_overrides=...`; TypeScript
`runSkill`/`streamSkill` accept `configOverrides`. Their keys match this schema's
YAML/JSON spelling; existing shorthand parameter casing remains unchanged.
Python model serialization excludes unset fields so defaults do not become
explicit overrides. Arrays in partial types remain complete replacements.

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
    # Also `stub: { output: ..., exit_code: 2 }` to fake a failing command, or
    # a list of either form for ordered responses (see "Ordered stub responses").
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

push = stub(pattern=r"git push\b", output="Everything up-to-date")
case = TestCase(
    skill="skills/greeter",          # relative to the working directory, not a file
    input="Greet Dr. Smith, who has an appointment today.",
    user=user("a terse patient", done_when="the appointment is confirmed"),  # optional
    mocks=[push],
    evals=[
        boolean("the reply greets Dr. Smith by name"),
        numeric("how warm is the tone", min=0, max=10, threshold=7),
        called(push, times=1),       # the mock object itself; a name= string also works
    ],
)
report = run_skill(case)
```

```ts
// TypeScript (@skill-test/sdk / @skill-test/vitest)
import { runSkill, testCase, boolean, numeric, called, stub, user } from "@skill-test/sdk";

const push = stub({ pattern: /git push\b/, output: "Everything up-to-date" });
const report = await runSkill(testCase({
  skill: "skills/greeter",
  input: "Greet Dr. Smith, who has an appointment today.",
  user: user("a terse patient", { doneWhen: "the appointment is confirmed" }),
  mocks: [push],
  evals: [
    boolean("the reply greets Dr. Smith by name"),
    numeric("how warm is the tone", { min: 0, max: 10, threshold: 7 }),
    called(push, { times: 1 }), // the mock object itself; a name string also works
  ],
}));
```

Under the hood the SDK serializes the case to JSON and runs `skilltest run
--case-json <file>`; unlike a YAML `PATH`, a code-defined case's `skill`
resolves relative to the **working directory** (the SDKs run the CLI from your
project). The builders construct models **generated from the input contract**
(`schemas/case.schema.json` — see "Input contract" below), so the payload shape
cannot drift from the Rust parse, and the CLI validates the compiled case
exactly as it validates YAML — a malformed case is a loud usage error, never a
vacuous pass. YAML files remain first-class — a path works everywhere a
code-defined case does, and the plugins still auto-discover `*.skilltest.yaml`
files.

### Ordered stub responses

A `stub` may also be a non-empty **list** of responses, each one a bare output
or an `{output, exit_code}` map. The *n*-th call the mock intercepts within one
run gets the *n*-th item, and every call after the last item gets the last item
again. A run is one platform × model run of the case, and a multi-turn case
counts across its whole conversation; every run starts again from the first
item, including runs executing at the same time. An item's `exit_code` applies
to that call alone. Here the first status check sees the job queued, the second
sees it fail, and every later check sees it pass:

```yaml
mocks:
  - name: status
    match: { tool: bash, contains: "jobctl status" }
    stub:
      - "build-42: queued"
      - { output: "build-42: failed (retrying)", exit_code: 1 }
      - "build-42: passed"
evals:
  - type: called
    mock: status
    times: 3
```

An empty list, an empty bare-string item, or an unknown key inside an item is
refused when the case loads. `called`/`not_called` and the report's
`mock_calls` record each intercepted call once with its original input and the
action `stub`, as for a single stub. In the SDKs the same mock is
`stub("jobctl status", responses=[...])` (Python) and
`stub({ contains: "jobctl status", responses: [...] })` (TypeScript, items
`{ output, exitCode }`).

oneharness's mock hook keeps no state between calls, so a list of two or more
items reaches the hook as a `rewrite` of the shell command. The rewritten
command takes the next number from a counter directory that skilltest creates
for that one run and removes when the run ends.

Unknown fields are rejected **everywhere** in a case — including inside an
eval, the `user` block, and the map forms of `stub`/`deny` and field
predicates — with an error naming the unknown key. A typo like `expcted:` can
therefore never silently fall back to a default and invert an eval's intent.

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

When the run was recorded to oneharness's history (the default oneharness
provider, `provider.history` on), each run also carries
**`history_command`**: a ready-to-run string such as `oneharness history show
<name> --history-dir <dir>` that replays the recorded transcript, so a past run
can be reviewed after the fact. It is `null` for providers/configs that record
no history. See [the protocol reference](protocol.md) for the centralized
history directory and how the session name is derived.

## Structured errors (`--format json` failures)

Exit codes 0 and 1 produce a `Report` (all passed / some failed). A run that
cannot produce a report at all — **bad input** (exit 2) or a **provider
failure** (exit 3) — instead emits a structured **`ReportError`** on stdout, so
a machine consumer gets the failure category without parsing stderr:

```json
{
  "code": "provider",
  "kind": "timeout",
  "context": "oneharness:claude-code",
  "message": "harness run failed: deadline exceeded"
}
```

- **`code`** — `usage` (exit 2) or `provider` (exit 3), mirroring the process
  exit code.
- **`kind`** — a `ProviderErrorKind` for a classified provider failure, else
  absent. The closed vocabulary is `auth`, `rate_limit`, `model_not_found`,
  `quota`, `overloaded`, `timeout`, `spawn`, `protocol`, `other` — the last is
  the forward-compatible catch-all for a failure this version can't map to a
  specific category. This is the field to branch on (retry a `timeout`, fail
  fast on `auth`) instead of matching substrings in `message`.
- **`context`** — the provider the failure came from (e.g.
  `oneharness:claude-code`, `api-judge`); absent for usage errors.
- **`message`** — the human description (the same text on stderr, minus the
  suggested-action hint the CLI also prints there).

The human hint still goes to stderr for the terminal; the structured object is
additive on a stdout channel that was previously empty on failure. Under
`--format json-stream` the same object is the terminal
`{"type":"error","error":{…}}` NDJSON line.

The SDKs surface it as a **kind-specific exception** carrying `.kind`/`.context`
— one subclass per concrete kind (`SkilltestTimeoutError`, `SkilltestAuthError`,
…), all extending `SkilltestProviderError`, so a handler catches one category
directly (`except SkilltestTimeoutError` / `catch (e) { if (e instanceof
SkilltestTimeoutError) }`) while a catch on the base still catches every provider
failure. The `other` catch-all and unclassified failures surface as the base
`SkilltestProviderError`. The subclass set can't drift from the Rust enum: in
TypeScript the kind→class registry is a `Record<ProviderErrorKind, …>` (a missing
kind fails `tsc`), and in Python a test ties the registry to the generated
`ProviderErrorKind` vocabulary.

## The contracts: how the CLI and the SDKs stay in sync

Two JSON contracts bind the CLI to the SDKs, both generated from the Rust
types by `scripts/gen-contract.sh` (`just gen-contract`) and both enforced by
the same drift gate:

- the **output contract** — the `--format json` report the SDKs parse
  (`schemas/report.schema.json`, `schemas/validation.schema.json`) plus the
  structured error emitted on a failure exit (`schemas/error.schema.json`,
  from which each SDK's error model — and the typed `SkilltestProviderError` —
  is generated), and
- the **input contract** — the test-case shape (`schemas/case.schema.json`,
  from `crates/skilltest-core/src/testcase.rs` + `eval.rs` + `mock.rs`), from
  which each SDK's *case* models are generated so the code-first case builders
  construct generated types and cannot emit a shape the Rust parse would
  reject or silently ignore.

The chain:

1. The types derive `schemars::JsonSchema`, and `skilltest schema
   <report|validation|case|error>` emits their JSON Schema (draft-07 on purpose
   — the dialect the generators below digest reliably).
2. The script writes those schemas to `schemas/` (the **goldens**), then
   generates each SDK's models from them:
   - Python: [`datamodel-code-generator`](https://github.com/koxudaxi/datamodel-code-generator)
     → Pydantic v2 models in `skilltest_sdk/_report.py` / `_validation.py` /
     `_case.py`, so Python keeps full runtime validation for free.
   - TypeScript: [`json-schema-to-typescript`](https://www.npmjs.com/package/json-schema-to-typescript)
     → type declarations in `src/generated/` (types only by design; the drift
     gate is what guarantees the shape, so the runner casts after `JSON.parse`
     instead of re-validating).
3. **Drift gate**: `just contract-check` (part of `just check`) regenerates
   everything into a staging dir and diffs it against the checked-in
   artifacts, so a contract change that skips regeneration — or a hand-edit of
   generated code — fails CI with the exact diff. A Rust e2e test additionally
   pins the binary to the checked-in goldens.
4. **Kitchen-sink golden** (input contract only):
   `tests/fixtures/contract/case_kitchen_sink.json` is one maximal case pinned
   by *three* parties — a Rust e2e test asserts a maximally-populated
   `TestCase` serializes to exactly it (and that it parses strictly and runs
   green against the fake provider), and each SDK asserts its case builders
   compile to exactly it. A field added to the contract forces the golden to
   change, which fails each SDK's pin until its builders can express the new
   field — additive drift is caught, not just breaking drift.

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
