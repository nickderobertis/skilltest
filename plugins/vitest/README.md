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

## Configuration

The plugin shells out to the `skilltest` binary. Point at one with the
`SKILLTEST_BIN` env var (or the `bin` option) and the provider with
`SKILLTEST_PROVIDER` (or the `provider` option). A failing eval is returned in
`report.passed`; bad input and provider failures throw `SkilltestUsageError` /
`SkilltestProviderError`. See the repository root for the provider protocol and
full schema.
