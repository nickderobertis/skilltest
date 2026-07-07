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

### Define the case in code (recommended)

Instead of a YAML file, build the whole case — skill, input, evals, an optional
simulated user, mocks — in code and pass it straight to `runSkill`. Everything
the YAML carries has a typed builder:

```ts
import { runSkill, testCase, boolean, numeric } from "@skill-test/sdk";

const report = await runSkill(
  testCase({
    skill: "skills/greeter",             // resolved relative to the working dir
    input: "Greet Dr. Smith, who has an appointment today.",
    evals: [
      boolean("the reply greets Dr. Smith by name"),
      numeric("how warm is the tone", { min: 0, max: 10, threshold: 7 }),
    ],
  }),
);
if (!report.passed) throw new Error(describeFailures(report));
```

Add `user(persona, { doneWhen })` for a multi-turn case, and a `mocks` array of
`stub`/`spy`/`deny`/`rewrite` (name them to reference from a `called` /
`notCalled` eval). A YAML path (`runSkill("cases/greet.yaml")`) works everywhere
a case object does; the field reference for both is
[`docs/schema.md`](../../docs/schema.md).

### Tool events

Each assistant turn carries the normalized tool events the skill took (shell
commands, file edits, tool uses), lifted from oneharness's `--events`. Assert on
*what the skill did* with `toolCalls` (the `tool_call` events across a transcript,
in order); each `ToolEvent` has `kind`, `name`, `input`, `output`, `index`:

```ts
import { runSkill, toolCalls } from "@skill-test/sdk";

const report = await runSkill("cases/edit.skilltest.yaml");
const calls = toolCalls(report.runs[0]!.transcript);
console.assert(calls.some((c) => String(c.input?.command).includes("git commit")));
console.assert(!calls.some((c) => String(c.input?.command).includes("rm -rf")));
```

### Streaming (opt-in)

`streamSkill` returns a `SkillStream` you iterate with `for await` to receive each
event live, and `break` to **short-circuit** — closing the stream tears the
harness down, so a bad turn is cut off instead of paid for in full. `.report`
holds the final report once the stream runs to completion:

```ts
import { streamSkill } from "@skill-test/sdk";

const stream = streamSkill("cases/edit.skilltest.yaml");
for await (const ev of stream) {   // ev: StreamEvent — .case/.platform/.model/.turn/.event
  if (ev.event.name === "bash" && String(ev.event.input?.command).includes("rm -rf")) break;
}
const report = stream.report;
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
