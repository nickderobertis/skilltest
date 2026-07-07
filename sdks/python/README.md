# skilltest-sdk

The Python SDK for the [`skilltest`](https://github.com/nickderobertis/skilltest)
CLI. A thin, typed wrapper and nothing else: it runs the CLI as a subprocess and
parses the stable `--format json` contract into Pydantic models. Test-framework
integrations build on it — use [`skilltest-pytest`](../../plugins/pytest) if you
want pytest collection; use this package directly from any other Python code.

```python
from skilltest_sdk import run_skill, validate_skill, describe_failures, assistant_text

report = run_skill("cases/greet.yaml")
assert report.passed, describe_failures(report)
# Mix in deterministic checks on the transcript:
assert "Dr. Smith" in assistant_text(report.runs[0].transcript)

result = validate_skill("skills/greeter")
assert result.valid
```

### Define the case in code (recommended)

Instead of a YAML file, build the whole case — skill, input, evals, an optional
simulated user, mocks — in code and pass it straight to `run_skill`. Everything
the YAML carries has a typed builder, so the case and its checks live in one
place:

```python
from skilltest_sdk import TestCase, run_skill, boolean, numeric

case = TestCase(
    skill="skills/greeter",              # resolved relative to the working dir
    input="Greet Dr. Smith, who has an appointment today.",
    evals=[
        boolean("the reply greets Dr. Smith by name"),
        numeric("how warm is the tone", min=0, max=10, threshold=7),
    ],
)
report = run_skill(case)
assert report.passed, describe_failures(report)
```

Add `user(persona=..., done_when=...)` for a multi-turn case, and `mocks=[...]`
of `stub`/`spy`/`deny`/`rewrite` (name them to reference from a `called` /
`not_called` eval). A YAML path (`run_skill("cases/greet.yaml")`) works
everywhere a `TestCase` does; the field reference for both is
[`docs/schema.md`](../../docs/schema.md).

### Tool events

Each assistant turn carries the normalized tool events the skill took (shell
commands, file edits, tool uses), lifted from oneharness's `--events`. Assert on
*what the skill did* with `tool_calls` (the `tool_call` events across a
transcript, in order); each `ToolEvent` has `kind`, `name`, `input`, `output`,
`index`:

```python
from skilltest_sdk import run_skill, tool_calls

report = run_skill("cases/edit.skilltest.yaml")
calls = tool_calls(report.runs[0].transcript)
assert any("git commit" in str(c.input) for c in calls)
assert not any("rm -rf" in str(c.input) for c in calls)   # never destructive
```

### Streaming (opt-in)

`stream_skill` returns a `SkillStream` you iterate with `async for` to receive
each event live, and `break` to **short-circuit** — closing the stream tears the
harness down, so a bad turn is cut off instead of paid for in full. `.report`
holds the final report once the stream runs to completion:

```python
from skilltest_sdk import stream_skill

async def guard():
    stream = stream_skill("cases/edit.skilltest.yaml")
    async for ev in stream:  # ev: StreamEvent — .case/.platform/.model/.turn/.event
        if ev.event.name == "bash" and "rm -rf" in str(ev.event.input):
            break
    return stream.report
```

The `skilltest` binary is resolved from the `bin=` argument, the
`SKILLTEST_BIN` env var, or `PATH`; a provider override comes from `provider=`
or `SKILLTEST_PROVIDER`. A failing eval is *reported* (`report.passed` is
false), not raised; bad input raises `SkilltestUsageError` (CLI exit 2) and
provider problems raise `SkilltestProviderError` (exit 3).

The Pydantic models are **generated** from `schemas/report.schema.json` /
`schemas/validation.schema.json` — themselves generated from the CLI's own
types — via `just gen-contract`, and a drift gate in CI fails if anything is
stale, so the models cannot diverge from the binary.
