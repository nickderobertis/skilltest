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
        ▲  SDKs + pytest / vitest packages                │ Provider
        │  (same JSON contract)                           ▼
        │                          ┌───────────────────────────────────────┐
   your test suite                 │ oneharness ──▶ claude-code / codex / … │
                                   │ (or a custom JSON-lines provider)      │
                                   └───────────────────────────────────────┘
```

## Install

Prebuilt binary (Linux/macOS, x86_64/aarch64) from the latest GitHub Release:

```bash
curl -fsSL https://raw.githubusercontent.com/nickderobertis/skilltest/main/scripts/install.sh | sh
```

Pin a version or install location with `SKILLTEST_VERSION` and
`SKILLTEST_INSTALL_DIR`; the script verifies the sha256 checksum before
installing.

### Build from source

skilltest is a Rust workspace with Python and TypeScript SDKs and
test-framework packages. You need
`cargo` (+ `cargo-nextest`), [`uv`](https://docs.astral.sh/uv/), and
`node`/[`pnpm`](https://pnpm.io), plus [`just`](https://github.com/casey/just).

```bash
just bootstrap   # cargo fetch + uv sync + pnpm install
just check       # the full quality gate (format, lint, types, unit + e2e)
```

See [`docs/development.md`](docs/development.md).

## Quick start

```bash
skilltest init            # scaffold skilltest.yaml + an example skill and case
skilltest run cases/example.yaml --provider skilltest-fake-provider   # try it offline
```

`skilltest init` writes a runnable starter project you can immediately run
against the bundled deterministic provider, then point at a real one.

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
skilltest run cases/greet.yaml                 # human summary (uses oneharness)
skilltest run cases/ --format json             # whole directory, machine output
skilltest run cases/greet.yaml -p claude-code -m sonnet
```

Multi-turn cases add a `user:` block with a persona and a `done_when` condition;
skilltest drives the simulated user until it holds (or `max_turns`).

Validate skill definitions:

```bash
skilltest validate skills/greeter      # a single skill
skilltest validate skills/             # a folder of skills
```

Scaffold a new project: `skilltest init [DIR]` writes a `skilltest.yaml`, an
example skill, and an example case (refusing to overwrite existing files).

Exit codes: `0` all passed · `1` a case/skill failed · `2` bad input ·
`3` provider failure.

## Use from your language and test runner

Same engine, surfaced as code so you can add deterministic checks alongside the
natural-language evals. Each language has one **SDK** that wraps the CLI and
nothing else — [`skilltest-sdk`](sdks/python) (Python, Pydantic models) and
[`@skill-test/sdk`](sdks/typescript) (TypeScript) — and one package per test
framework built on it, which re-exports the SDK so a test suite needs a single
dependency. SDK models are generated from the CLI's own JSON Schemas, so they
cannot drift from the binary.

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
import { runSkill, assistantText } from "@skill-test/vitest";

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
- **`sdks/{python,typescript}`** — one SDK per language: a thin, typed wrapper
  over that JSON contract, with models generated from the golden schemas in
  `schemas/` (themselves generated from the Rust types) and a CI drift gate —
  see [`docs/schema.md`](docs/schema.md).
- **`plugins/{pytest,vitest}`** — one package per test framework, built on its
  language's SDK.

The boundary to a model is the `Provider` trait ([`docs/protocol.md`](docs/protocol.md))
with two backends: the default **oneharness** provider runs each skill on a
harness (Claude Code, Codex, …) by passing the skill via `--system`, threading
`session_id` through `--resume` for faithful multi-turn on supporting harnesses,
and surfacing each result's normalized `usage` (token + cost totals) and
`failure_kind` (auth / rate-limit / … classification). A **custom command**
provider speaks a small JSON-lines protocol (this is how the deterministic
`skilltest-fake-provider` keeps the test gate model-free). Today the lineup is
Python/pytest and TypeScript/vitest; adding a language means one new SDK under
`sdks/`, and adding a test framework means one new package under `plugins/`.

## License

MIT — see [`LICENSE`](LICENSE).
