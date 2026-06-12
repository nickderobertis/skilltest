# @skilltest/sdk

The TypeScript SDK for the
[`skilltest`](https://github.com/nickderobertis/skilltest) CLI. A thin, typed
wrapper and nothing else: it runs the CLI as a subprocess and parses the stable
`--format json` contract with Zod. Test-framework integrations build on it —
use [`@skilltest/vitest`](../../plugins/vitest) if you want the vitest helpers;
use this package directly from any other TypeScript/JavaScript code.

```ts
import { runSkill, validateSkill, assistantText, describeFailures } from "@skilltest/sdk";

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

The Zod schemas mirror `schemas/report.schema.json` /
`schemas/validation.schema.json` (generated from the CLI's own types); a
contract test in this package fails if they drift.
