# @skilltest/vitest

A [vitest](https://vitest.dev) plugin for [skilltest](../../README.md): run
AI-skill tests and natural-language evals from vitest, and mix in your own
deterministic checks.

## As code

```ts
import { runSkill, assistantText } from "@skilltest/vitest";

test("greeter names the patient", async () => {
  const report = await runSkill("cases/greet.yaml", {
    platforms: ["claude-code"],
    models: ["claude-opus-4-8"],
  });
  expect(report.passed, report.runs[0]?.case).toBe(true);
  expect(assistantText(report.runs[0]!.transcript)).toContain("Dr. Smith");
});
```

## One-liner

```ts
import { skillTest } from "@skilltest/vitest/vitest";

skillTest("greeter confirms the appointment", "cases/greet.yaml");
```

## Recommended: auto-discover a tree of cases

When vitest is your primary test runner, keep your cases as data and let one
test module collect them. Name each case `*.skilltest.yaml` (or `.yml`) and add
a single `skills.test.ts`:

```ts
// skills.test.ts
import { discover } from "@skilltest/vitest/vitest";

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
["claude-code"] })`); for matrices or deterministic mix-in assertions, reach for
`runSkill` in an ordinary `test()` instead.

## Configuration

The plugin shells out to the `skilltest` binary. Point at one with the
`SKILLTEST_BIN` env var (or the `bin` option) and the provider with
`SKILLTEST_PROVIDER` (or the `provider` option). A failing eval is returned in
`report.passed`; bad input and provider failures throw `SkilltestUsageError` /
`SkilltestProviderError`. See the repository root for the provider protocol and
full schema.
