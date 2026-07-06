# skilltest-pytest

A [pytest](https://pytest.org) plugin for [skilltest](../../README.md): run
AI-skill tests and natural-language evals as ordinary pytest tests, and mix in
your own deterministic checks. Built on
[`skilltest-sdk`](../../sdks/python/README.md) — the SDK's code API is
re-exported here, so a pytest suite needs only this one dependency.

## Two ways to use it

**Auto-collected case files.** Name a case `something.skilltest.yaml` and pytest
runs it:

```yaml
# greet.skilltest.yaml
skill: ./skills/greeter
input: "Greet Dr. Smith."
evals:
  - type: boolean
    criterion: "the reply greets Dr. Smith by name"
```

**As code**, for matrices and deterministic mix-ins:

```python
from skilltest_pytest import run_skill, describe_failures, assistant_text

def test_greeter():
    report = run_skill("cases/greet.yaml", platforms=["claude-code"], models=["claude-opus-4-8"])
    assert report.passed, describe_failures(report)
    assert "Dr. Smith" in assistant_text(report.runs[0].transcript)
```

## Assert on tool use, and stream

The SDK's tool-event and streaming surfaces are re-exported too. `tool_calls`
returns the normalized `tool_call` events a run took (each a `ToolEvent` with
`kind`/`name`/`input`/`output`/`index`), and `stream_skill` yields them live so a
test can **short-circuit** on bad behavior:

```python
from skilltest_pytest import run_skill, tool_calls

def test_commits_but_never_deletes():
    report = run_skill("cases/edit.skilltest.yaml")
    calls = tool_calls(report.runs[0].transcript)
    assert any("git commit" in str(c.input) for c in calls)
    assert not any("rm -rf" in str(c.input) for c in calls)
```

```python
import asyncio
from skilltest_pytest import stream_skill

def test_makes_no_network_call():
    async def go():
        async for ev in stream_skill("cases/edit.skilltest.yaml"):
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
