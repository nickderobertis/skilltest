import { beforeAll, expect, it } from "vitest";
// One dependency is enough for a vitest suite: the SDK's code-level API is
// re-exported straight from @skill-test/vitest.
import { assistantText, describeFailures, runSkill } from "../src/index.js";
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
