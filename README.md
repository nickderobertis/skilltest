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

**Mock and spy on tool calls** (sinon's vocabulary, delivered per run through
each harness's own hook protocol — no permanent config mutation): a `mocks:`
entry with a `stub`/`deny`/`rewrite` action intercepts matching calls inside
the real harness; one without an action is a spy that only observes. The
deterministic `called`/`not_called` evals then assert on what the skill
*attempted* — no judge, no flakiness:

```yaml
mocks:
  - name: push
    match: { tool: bash, pattern: "git push( --force)?\\b" }
    stub: Everything up-to-date        # canned result; nothing real runs
  - name: danger
    match: { contains: "rm -rf" }
    deny: destructive commands are blocked
evals:
  - type: boolean
    criterion: "reports the deploy as already up to date"
  - type: called
    mock: push
    times: 1
  - type: not_called
    mock: danger
```

The report's `mock_calls` records every observed call with its **original**
input and verdict (the transcript's `events` show what actually ran instead).
A harness that cannot express a requested verb fails loudly, never silently.

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

**pytest** ([`plugins/pytest`](plugins/pytest)) — define the whole case in code
(the recommended form) and mix in deterministic checks; a YAML path works too,
and `*.skilltest.yaml` files are auto-collected:

```python
from skilltest_pytest import TestCase, run_skill, boolean, numeric, describe_failures, assistant_text

def test_greeter():
    case = TestCase(
        skill="skills/greeter",
        input="Greet Dr. Smith, who has an appointment today.",
        evals=[
            boolean("the reply greets Dr. Smith by name"),
            numeric("how warm is the tone", min=0, max=10, threshold=7),
        ],
    )
    report = run_skill(case)  # or run_skill("cases/greet.yaml")
    assert report.passed, describe_failures(report)
    assert "Dr. Smith" in assistant_text(report.runs[0].transcript)
```

Code-level mocks and spies are objects you hold and assert on directly
(`vi.fn()`/`unittest.mock` semantics, without the exact-args defaults or the
typo-swallowing `Mock`):

```python
from skilltest_pytest import TestCase, run_skill, spy, stub, boolean, contains, matching

DEPLOY = TestCase(
    skill="skills/deployer",
    input="Deploy the app",
    evals=[boolean("the deploy is reported successful")],
)

def test_deploy_is_mocked():
    push = stub(pattern=r"git push( --force)?\b", output="Everything up-to-date")
    git = spy(tool="bash", pattern=r"\bgit\b")

    run_skill(DEPLOY, mocks=[push, git])

    push.assert_called_once()
    assert "origin" in push.calls[0].command          # original, pre-rewrite input
    git.assert_called_with(command=contains("git status"))
    git.where(command=matching(r"\bsudo\b")).assert_not_called()
```

**vitest** ([`plugins/vitest`](plugins/vitest)) — same, with a `testCase({...})`
object (or a YAML path):

```ts
import { runSkill, testCase, boolean, numeric, assistantText } from "@skill-test/vitest";

test("greeter", async () => {
  const report = await runSkill(testCase({
    skill: "skills/greeter",
    input: "Greet Dr. Smith, who has an appointment today.",
    evals: [
      boolean("the reply greets Dr. Smith by name"),
      numeric("how warm is the tone", { min: 0, max: 10, threshold: 7 }),
    ],
  }));
  expect(report.passed).toBe(true);
  expect(assistantText(report.runs[0]!.transcript)).toContain("Dr. Smith");
});
```

```ts
import { runSkill, testCase, spy, stub, boolean, matching } from "@skill-test/vitest";

const deploy = testCase({
  skill: "skills/deployer",
  input: "Deploy the app",
  evals: [boolean("the deploy is reported successful")],
});

test("deploy is mocked", async () => {
  const push = stub({ pattern: /git push( --force)?\b/, output: "Everything up-to-date" });
  const git = spy({ tool: "bash", pattern: /\bgit\b/ });

  await runSkill(deploy, { mocks: [push, git] });

  expect(push.callCount).toBe(1);
  expect(push.calls[0]!.command).toContain("origin"); // original, pre-rewrite input
  expect(git.where({ command: matching(/\bsudo\b/) }).called).toBe(false);
});
```

A **mock** compiles into the hook-side ruleset that runs inside the harness
(Rust-regex, `contains()`/`matching()` predicates only); a **spy** filters the
returned records locally, so its patterns are your language's native regex and
`where()` accepts arbitrary predicates. Reading a spy that was never passed to
a run throws — "no run yet" never reads as "zero calls".

### Inspect tool use, and stream

Each assistant turn also carries the **normalized tool events** the skill took —
shell commands, file edits, tool uses — surfaced across every harness by
oneharness's `--events` and exposed as `tool_calls`/`toolCalls`. Assert on *what
the skill did*, not just what it said:

```python
from skilltest_pytest import TestCase, run_skill, tool_calls, boolean

case = TestCase(
    skill="skills/editor",
    input="Update the config and commit it.",
    evals=[boolean("the change was committed")],
)
report = run_skill(case)
calls = tool_calls(report.runs[0].transcript)          # the tool_call events, in order
assert any("git commit" in str(c.input) for c in calls)
assert not any("rm -rf" in str(c.input) for c in calls)
```

For long runs, an opt-in **streaming** API yields those events live and lets you
**short-circuit** the moment bad behavior appears — closing the stream tears the
harness down, so a bad turn is cut off instead of paid for in full:

```python
from skilltest_pytest import stream_skill

async def guard(case):
    stream = stream_skill(case)                        # a TestCase or a YAML path
    async for ev in stream:                            # ev.event is a ToolEvent
        if ev.event.name == "bash" and "rm -rf" in str(ev.event.input):
            break                                      # abort the run now
    return stream.report                               # the full report, if it ran to completion
```

TypeScript mirrors both — `toolCalls(transcript)` and `streamSkill(...)`
(a `for await` of events; `break` to short-circuit). Note the two views differ
under mocking: `events` show post-rewrite reality (the stub that actually ran),
while a mock/spy's `.calls` keep the skill's original attempt.

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
with two backends: the default **oneharness** provider (v0.3.6+) runs each skill
on a harness (Claude Code, Codex, …) by passing the skill via `--system`,
threading `session_id` through `--resume` for faithful multi-turn on supporting
harnesses, lifting normalized tool events onto each turn via `--events` (and
`--stream` for the live streaming API), and surfacing each result's normalized
`usage` (token + cost totals) and `failure_kind` (auth / rate-limit / …
classification). It passes no `--mode`, so oneharness's default approval mode
applies — set `ONEHARNESS_MODE=bypass` (via oneharness config) to let the skill
take every action without prompting. A **custom command** provider speaks a small
JSON-lines protocol (this is how the deterministic `skilltest-fake-provider`
keeps the test gate model-free). Today the lineup is Python/pytest and
TypeScript/vitest; adding a language means one new SDK under `sdks/`, and adding a
test framework means one new package under `plugins/`.

## License

MIT — see [`LICENSE`](LICENSE).
