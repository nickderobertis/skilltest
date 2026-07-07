# skilltest-pytest

A [pytest](https://pytest.org) plugin for [skilltest](../../README.md): run
AI-skill tests and natural-language evals as ordinary pytest tests, and mix in
your own deterministic checks. Built on
[`skilltest-sdk`](../../sdks/python/README.md) — the SDK's code API is
re-exported here, so a pytest suite needs only this one dependency.

## Define the whole case in code (recommended)

Build the case — skill, input, evals, an optional simulated user, mocks — right
in the test. Everything the YAML carries has a typed builder, so the case, its
mocks, and any deterministic transcript checks live in one place:

```python
from skilltest_pytest import TestCase, run_skill, boolean, numeric, describe_failures

def test_greeter():
    case = TestCase(
        skill="skills/greeter",           # resolved relative to the working dir
        input="Greet Dr. Smith, who has an appointment today.",
        evals=[
            boolean("the reply greets Dr. Smith by name"),
            numeric("how warm is the tone", min=0, max=10, threshold=7),
        ],
    )
    report = run_skill(case, platforms=["claude-code"], models=["claude-opus-4-8"])
    assert report.passed, describe_failures(report)
```

Multi-turn cases add `user(...)`; deterministic call-count checks use `called` /
`not_called` taking the `stub`/`spy` object itself or a name you gave it (or
use the mock objects' own assertions — see below). `run_skill` also takes `platforms=`/`models=` to fan a
case across a matrix.

## Or point at a YAML file

`run_skill` accepts a path just as well (`run_skill("cases/greet.yaml")`), and
**auto-collection** still works: name a case `something.skilltest.yaml` and
pytest runs it with no test function at all —

```yaml
# greet.skilltest.yaml
skill: ./skills/greeter
input: "Greet Dr. Smith."
evals:
  - type: boolean
    criterion: "the reply greets Dr. Smith by name"
```

The full field reference for both forms is [`docs/schema.md`](../../docs/schema.md).

## Assert on tool use, and stream

The SDK's tool-event and streaming surfaces are re-exported too. `tool_calls`
returns the normalized `tool_call` events a run took (each a `ToolEvent` with
`kind`/`name`/`input`/`output`/`index`), and `stream_skill` yields them live so a
test can **short-circuit** on bad behavior:

```python
from skilltest_pytest import TestCase, run_skill, tool_calls, boolean

EDIT_CASE = TestCase(
    skill="skills/editor",
    input="Update the config and commit it.",
    evals=[boolean("the change was committed")],
)

def test_commits_but_never_deletes():
    report = run_skill(EDIT_CASE)
    calls = tool_calls(report.runs[0].transcript)
    assert any("git commit" in str(c.input) for c in calls)
    assert not any("rm -rf" in str(c.input) for c in calls)
```

```python
import asyncio
from skilltest_pytest import stream_skill

def test_makes_no_network_call():
    async def go():
        async for ev in stream_skill(EDIT_CASE):
            assert ev.event.name != "curl", "skill made a network call"
    asyncio.run(go())   # or use pytest-asyncio and `async def test_...`
```

## Configuration

The plugin shells out to the `skilltest` binary. Point it at one with the
`SKILLTEST_BIN` env var (or `bin=`), the provider with `SKILLTEST_PROVIDER` (or
`provider=`), and set defaults in `pyproject.toml`:

```toml
[tool.pytest.ini_options]
skilltest_provider = "oneharness"
skilltest_platforms = ["claude-code"]
skilltest_models = ["claude-opus-4-8"]
```

See the repository root for the provider protocol and the full schema.
