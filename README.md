# skilltest

A framework for **testing AI skills**. Give a skill (a `SKILL.md` plus its
assets) some starting input, optionally drive a simulated user across several
turns, then score the resulting transcript with built-in **natural-language
evals** — boolean assertions and numeric scores. Run it as a CLI, or from inside
`pytest` or `vitest` where you can mix in your own deterministic checks.

skilltest runs skills on different harness/model **platforms** through
[`oneharness`](https://github.com/nickderobertis/oneharness), so the same test
suite can compare a skill across, say, `claude-code` and `cursor`, or across
models.

```
┌──────────────┐   YAML cases + config    ┌───────────────────────────────┐
│  skilltest   │ ───────────────────────▶ │ skilltest-core                │
│  CLI         │                          │  load → converse → eval → report
└──────────────┘                          └───────────────┬───────────────┘
        ▲  pytest / vitest plugins                        │ provider protocol (JSON)
        │  (same JSON contract)                           ▼
        │                                  ┌───────────────────────────────┐
   your test suite                         │ oneharness  (or any provider) │
                                           └───────────────────────────────┘
```

## Install / build

skilltest is a Rust workspace with Python and TypeScript plugins. You need
`cargo` (+ `cargo-nextest`), [`uv`](https://docs.astral.sh/uv/), and
`node`/[`pnpm`](https://pnpm.io), plus [`just`](https://github.com/casey/just).

```bash
just bootstrap   # cargo fetch + uv sync + pnpm install
just check       # the full quality gate (format, lint, types, unit + e2e)
```

See [`docs/development.md`](docs/development.md).

## Use the CLI

Write a test case (full schema in [`docs/schema.md`](docs/schema.md)):

```yaml
# cases/greet.yaml
skill: ../skills/greeter
input: "Greet Dr. Smith, who has an appointment today."
evals:
  - type: boolean
    criterion: "the reply greets Dr. Smith by name"
  - type: numeric
    criterion: "how warm and professional is the greeting"
    min: 0
    max: 10
    threshold: 7
```

Run it across the platforms/models in your `skilltest.yaml`:

```bash
skilltest run cases/greet.yaml                 # human summary
skilltest run cases/ --format json             # whole directory, machine output
skilltest run cases/greet.yaml -p claude-code -m claude-opus-4-8
```

Multi-turn cases add a `user:` block with a persona and a `done_when` condition;
skilltest drives the simulated user until it holds (or `max_turns`).

Validate skill definitions:

```bash
skilltest validate skills/greeter      # a single skill
skilltest validate skills/             # a folder of skills
```

Exit codes: `0` all passed · `1` a case/skill failed · `2` bad input ·
`3` provider failure.

## Use as a plugin

Same engine, surfaced as code in your existing test runner so you can add
deterministic checks alongside the natural-language evals.

**pytest** ([`plugins/pytest`](plugins/pytest)) — auto-collects
`*.skilltest.yaml`, or call the API:

```python
from skilltest_pytest import run_skill

def test_greeter():
    report = run_skill("cases/greet.yaml")
    assert report.passed, report.describe_failures()
    assert "Dr. Smith" in report.runs[0].transcript.assistant_text()
```

**vitest** ([`plugins/vitest`](plugins/vitest)):

```ts
import { runSkill, assistantText } from "@skilltest/vitest";

test("greeter", async () => {
  const report = await runSkill("cases/greet.yaml");
  expect(report.passed).toBe(true);
  expect(assistantText(report.runs[0]!.transcript)).toContain("Dr. Smith");
});
```

## How it works

- **`crates/skilltest-core`** — the engine: config, skill model + validation,
  test-case model, the provider protocol, evals, the conversation runner, and
  the report. Its `--format json` output is a stable contract.
- **`crates/skilltest-cli`** — the `skilltest` binary, plus
  `skilltest-fake-provider`, a deterministic reference provider that lets the
  whole pipeline be tested without a live model.
- **`plugins/{pytest,vitest}`** — thin, typed wrappers over that JSON contract.

The boundary to a model is a small JSON protocol
([`docs/protocol.md`](docs/protocol.md)); `oneharness` is the default
implementation, but any command that speaks it works. Today the plugin lineup is
pytest and vitest; the architecture is built to grow to other languages and test
frameworks.

## License

MIT — see [`LICENSE`](LICENSE).
