# @skill-test/sdk

The TypeScript SDK for the
[`skilltest`](https://github.com/nickderobertis/skilltest) CLI. A thin, typed
wrapper and nothing else: it runs the CLI as a subprocess and types the stable
`--format json` contract with declarations generated from the CLI's own JSON
Schemas. Test-framework integrations build on it — use
[`@skill-test/vitest`](../../plugins/vitest) if you want the vitest helpers; use
this package directly from any other TypeScript/JavaScript code.

Define the whole case — skill, input, evals, an optional simulated user, mocks —
in code and pass it straight to `runSkill`. Everything a case YAML carries has a
typed builder, so the case and its checks live in one place:

```ts
import { runSkill, testCase, boolean, numeric, assistantText, describeFailures } from "@skill-test/sdk";

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
// Mix in deterministic checks on the transcript:
const text = assistantText(report.runs[0]!.transcript);
```

Add `user(persona, { doneWhen })` for a multi-turn case, and a `mocks` array of
`stub`/`spy`/`deny`/`rewrite` (a `called` / `notCalled` eval takes the mock
object itself, or a `name` you gave it). Validate a skill definition with
`validateSkill("skills/greeter")`.

### Or point at existing YAML cases

`runSkill` also takes a path to a test-case YAML file — or a directory of
them — so a suite that keeps cases as data works unchanged, and the
[vitest plugin](../../plugins/vitest)'s `discover` collects a whole tree of
`*.skilltest.yaml` files in one line:

```ts
const report = await runSkill("cases/greet.yaml"); // or runSkill("cases/")
```

The field reference for both forms is [`docs/schema.md`](../../docs/schema.md).

### Tool events

Each assistant turn carries the normalized tool events the skill took (shell
commands, file edits, tool uses), lifted from oneharness's `--events`. Assert on
*what the skill did* with `toolCalls` (the `tool_call` events across a transcript,
in order); each `ToolEvent` has `kind`, `name`, `input`, `output`, `index`:

```ts
import { runSkill, testCase, toolCalls, boolean } from "@skill-test/sdk";

const report = await runSkill(
  testCase({
    skill: "skills/editor",
    input: "Update the config and commit it.",
    evals: [boolean("the change was committed")],
  }),
);
const calls = toolCalls(report.runs[0]!.transcript);
console.assert(calls.some((c) => String(c.input?.command).includes("git commit")));
console.assert(!calls.some((c) => String(c.input?.command).includes("rm -rf")));
```

### Streaming (opt-in)

`streamSkill` takes the same case (or path) and returns a `SkillStream` you
iterate with `for await` to receive each event live, and `break` to
**short-circuit** — closing the stream tears the harness down, so a bad turn is
cut off instead of paid for in full. `.report` holds the final report once the
stream runs to completion:

```ts
import { streamSkill } from "@skill-test/sdk";

const stream = streamSkill(myCase);
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

The types in `src/generated/` are **generated** from the golden schemas in
`schemas/` — themselves generated from the CLI's own types — via `just
gen-contract`, and a drift gate in CI fails if anything is stale, so the types
cannot diverge from the binary. That covers both directions: the report types
the SDK parses (`generated/report.ts`) and the case types the builders
construct (`generated/case.ts`). They are types only: the runner trusts the
shape after `JSON.parse`, because the gate (not runtime re-validation) is what
guarantees it.
