# @skill-test/vitest

A [vitest](https://vitest.dev) plugin for [skilltest](../../README.md): run
AI-skill tests and natural-language evals from vitest, and mix in your own
deterministic checks. Built on
[`@skill-test/sdk`](../../sdks/typescript/README.md) — the SDK's API is
re-exported here, so a vitest suite needs only this one dependency.

## Define the whole case in code (recommended)

Build the case — skill, input, evals, an optional simulated user, mocks — right
in the test, and register it in one line with `skillTest`. Everything the YAML
carries has a typed builder:

```ts
import { skillTest, testCase, boolean, numeric } from "@skill-test/vitest";

skillTest(
  "greeter names the patient",
  testCase({
    skill: "skills/greeter",            // resolved relative to the working dir
    input: "Greet Dr. Smith, who has an appointment today.",
    evals: [
      boolean("the reply greets Dr. Smith by name"),
      numeric("how warm is the tone", { min: 0, max: 10, threshold: 7 }),
    ],
  }),
);
```

Multi-turn cases add `user(...)`; call-count checks use `called` / `notCalled`
taking the `stub`/`spy` object itself or a name you gave it. For a matrix or extra deterministic checks,
call `runSkill` from an ordinary `test` — it takes the same case object:

```ts
import { runSkill, testCase, boolean, assistantText } from "@skill-test/vitest";

test("greeter across the matrix", async () => {
  const report = await runSkill(
    testCase({ skill: "skills/greeter", input: "Greet Dr. Smith", evals: [boolean("greets by name")] }),
    { platforms: ["claude-code"], models: ["claude-opus-4-8"] },
  );
  expect(report.passed).toBe(true);
  expect(assistantText(report.runs[0]!.transcript)).toContain("Dr. Smith");
});
```

## Or point at a YAML file

`skillTest` and `runSkill` accept a path just as well
(`skillTest("greeter", "cases/greet.yaml")`). The full field reference for both
forms is [`docs/schema.md`](../../docs/schema.md).

## Assert on tool use, and stream

The SDK's tool-event and streaming surfaces are re-exported too. `toolCalls`
returns the normalized `tool_call` events a run took (each a `ToolEvent` with
`kind`/`name`/`input`/`output`/`index`), and `streamSkill` yields them live so a
test can **short-circuit** on bad behavior:

```ts
import { it, expect } from "vitest";
import { runSkill, streamSkill, testCase, toolCalls, boolean } from "@skill-test/vitest";

const editCase = testCase({
  skill: "skills/editor",
  input: "Update the config and commit it.",
  evals: [boolean("the change was committed")],
});

it("commits without deleting", async () => {
  const report = await runSkill(editCase);
  const calls = toolCalls(report.runs[0]!.transcript);
  expect(calls.some((c) => String(c.input?.command).includes("git commit"))).toBe(true);
  expect(calls.some((c) => String(c.input?.command).includes("rm -rf"))).toBe(false);
});

it("makes no network call", async () => {
  for await (const ev of streamSkill(editCase)) {
    expect(ev.event.name).not.toBe("curl"); // break to abort early
  }
});
```

## Auto-discover existing YAML cases

Cases can also live as data: name each `*.skilltest.yaml` (or `.yml`) and let
one test module collect the whole tree — useful when a suite already has YAML
cases, or when non-engineers author them:

```ts
// skills.test.ts
import { discover } from "@skill-test/vitest";

discover("cases"); // registers one vitest test per *.skilltest.yaml under cases/
```

```yaml
# cases/greet.skilltest.yaml
skill: ./skills/greeter
input: "Greet Dr. Smith."
evals:
  - type: boolean
    criterion: "the reply greets Dr. Smith by name"
```

This is the closest vitest equivalent to pytest's auto-collection: vitest only
collects its own test modules, so the one-line `discover()` call stands in for a
file collector. Adding a case is then just dropping in a YAML file — no code
change. Pass run options as the second argument (`discover("cases", { platforms:
["claude-code"] })`). YAML cases and code-defined ones mix freely in one suite.

## Configuration

The plugin shells out to the `skilltest` binary. Point at one with the
`SKILLTEST_BIN` env var (or the `bin` option) and the provider with
`SKILLTEST_PROVIDER` (or the `provider` option). A failing eval is returned in
`report.passed`; bad input and provider failures throw `SkilltestUsageError` /
`SkilltestProviderError`. See the repository root for the provider protocol and
full schema.
