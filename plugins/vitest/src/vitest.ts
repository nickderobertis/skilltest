/**
 * vitest integration helper. Import from `@skilltest/vitest/vitest` to register
 * a skill case as a vitest test in one line:
 *
 * ```ts
 * import { skillTest } from "@skilltest/vitest/vitest";
 * skillTest("greeter names the patient", "cases/greet.yaml");
 * ```
 *
 * For matrices or deterministic mix-in checks, call {@link runSkill} from an
 * ordinary `test()` instead. This module imports `vitest`, so only load it
 * inside a vitest run.
 */
import { expect, test } from "vitest";
import type { RunOptions } from "./runner.js";
import { runSkill } from "./runner.js";
import { describeFailures } from "./schema.js";

/** Register a vitest test that runs `casePath` and asserts every eval passed. */
export function skillTest(name: string, casePath: string, options: RunOptions = {}): void {
  test(name, async () => {
    const report = await runSkill(casePath, options);
    expect(report.passed, describeFailures(report)).toBe(true);
  });
}
