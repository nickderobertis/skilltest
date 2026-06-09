# skilltest-pytest

A [pytest](https://pytest.org) plugin for [skilltest](../../README.md): run
AI-skill tests and natural-language evals as ordinary pytest tests, and mix in
your own deterministic checks.

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
from skilltest_pytest import run_skill

def test_greeter():
    report = run_skill("cases/greet.yaml", platforms=["claude-code"], models=["claude-opus-4-8"])
    assert report.passed, report.describe_failures()
    assert "Dr. Smith" in report.runs[0].transcript.assistant_text()
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
