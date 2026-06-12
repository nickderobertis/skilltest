# @skill-test/sdk

The TypeScript SDK for the
[`skilltest`](https://github.com/nickderobertis/skilltest) CLI. A thin, typed
wrapper and nothing else: it runs the CLI as a subprocess and types the stable
`--format json` contract with declarations generated from the CLI's own JSON
Schemas. Test-framework integrations build on it — use
[`@skill-test/vitest`](../../plugins/vitest) if you want the vitest helpers; use
this package directly from any other TypeScript/JavaScript code.

```ts
import { runSkill, validateSkill, assistantText, describeFailures } from "@skill-test/sdk";

const report = await runSkill("cases/greet.yaml");
if (!report.passed) throw new Error(describeFailures(report));
// Mix in deterministic checks on the transcript:
const text = assistantText(report.runs[0]!.transcript);

const result = await validateSkill("skills/greeter");
```

The `skilltest` binary is resolved from the `bin` option, the `SKILLTEST_BIN`
env var, or `PATH`; a provider override comes from `provider` or
`SKILLTEST_PROVIDER`. A failing eval is *reported* (`report.passed` is false),
not thrown; bad input throws `SkilltestUsageError` (CLI exit 2) and provider
problems throw `SkilltestProviderError` (exit 3).

The types in `src/generated/` are **generated** from
`schemas/report.schema.json` / `schemas/validation.schema.json` — themselves
generated from the CLI's own types — via `just gen-contract`, and a drift gate
in CI fails if anything is stale, so the types cannot diverge from the binary.
They are types only: the runner trusts the shape after `JSON.parse`, because
the gate (not runtime re-validation) is what guarantees it.
