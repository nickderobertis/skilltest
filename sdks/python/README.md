# skilltest-sdk

The Python SDK for the [`skilltest`](https://github.com/nickderobertis/skilltest)
CLI. A thin, typed wrapper and nothing else: it runs the CLI as a subprocess and
parses the stable `--format json` contract into Pydantic models. Test-framework
integrations build on it — use [`skilltest-pytest`](../../plugins/pytest) if you
want pytest collection; use this package directly from any other Python code.

```python
from skilltest_sdk import run_skill, validate_skill

report = run_skill("cases/greet.yaml")
assert report.passed, report.describe_failures()
# Mix in deterministic checks on the transcript:
assert "Dr. Smith" in report.runs[0].transcript.assistant_text()

result = validate_skill("skills/greeter")
assert result.valid
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
