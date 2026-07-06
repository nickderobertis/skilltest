import { beforeAll, expect, it } from "vitest";
// One dependency is enough for a vitest suite: the SDK's code-level API is
// re-exported straight from @skill-test/vitest.
import { assistantText, describeFailures, runSkill, streamSkill, toolCalls } from "../src/index.js";
import { caseFile, requireBinaries } from "./helpers.js";

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
