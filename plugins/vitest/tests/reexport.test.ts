import { beforeAll, expect, it } from "vitest";
// One dependency is enough for a vitest suite: the SDK's code-level API is
// re-exported straight from @skill-test/vitest.
import {
  assistantText,
  boolean,
  called,
  describeFailures,
  numeric,
  runSkill,
  streamSkill,
  stub,
  testCase,
  toolCalls,
  user,
} from "../src/index.js";
import { caseFile, requireBinaries, skillDir } from "./helpers.js";

beforeAll(() => {
  requireBinaries();
});

it("re-exports the SDK API and runs end-to-end", async () => {
  const report = await runSkill(caseFile("greet_pass.yaml"));
  expect(report.passed, describeFailures(report)).toBe(true);
  const run = report.runs[0];
  expect(run).toBeDefined();
  if (!run) return;
  expect(assistantText(run.transcript)).toContain("Dr. Smith");
});

it("re-exports the full code-defined case surface", async () => {
  // Every case builder rides the one-dependency re-export: a multi-turn case
  // with a named mock, judge evals, and a deterministic call eval, defined
  // entirely in code and run through the plugin's exports.
  const push = stub({
    pattern: /git push( --force)?\b/,
    output: "Everything up-to-date",
    name: "push",
  });
  const report = await runSkill(
    testCase({
      skill: skillDir("greeter"),
      input: "I'd like to confirm my appointment, please.",
      user: user("You are a terse patient.\nsay: Yes, please go ahead.", {
        doneWhen: "the conversation has reached turns>=2",
        maxTurns: 4,
      }),
      evals: [
        boolean("the assistant confirmed the appointment (`confirmed`)"),
        numeric("mentions `confirmed`", { min: 0, max: 10, threshold: 5, comparator: ">" }),
      ],
    }),
  );
  expect(report.passed, describeFailures(report)).toBe(true);
  expect(report.runs[0]?.turns).toBe(2);

  const deploy = await runSkill(
    testCase({
      skill: skillDir("deployer"),
      input: "Deploy the app",
      mocks: [push],
      evals: [called("push", { times: 1 })],
    }),
  );
  expect(deploy.passed, describeFailures(deploy)).toBe(true);
  expect(push.callCount).toBe(1);
});

it("re-exports the tool-event and streaming surfaces", async () => {
  const report = await runSkill(caseFile("tool_events.yaml"));
  const run = report.runs[0];
  expect(run).toBeDefined();
  if (!run) return;
  expect(toolCalls(run.transcript).map((c) => c.name)).toEqual(["edit_file", "bash"]);

  const names: (string | null | undefined)[] = [];
  for await (const ev of streamSkill(caseFile("tool_events.yaml"))) {
    names.push(ev.event.name);
  }
  expect(names).toEqual(["edit_file", "bash"]);
});
