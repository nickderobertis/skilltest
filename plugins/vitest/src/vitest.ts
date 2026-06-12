/**
 * vitest integration helper. Import from `@skilltest/vitest/vitest` to register
 * a skill case as a vitest test in one line:
 *
 * ```ts
 * import { skillTest } from "@skilltest/vitest/vitest";
 * skillTest("greeter names the patient", "cases/greet.yaml");
 * ```
 *
 * Or auto-discover a tree of cases (the recommended setup when vitest is your
 * primary runner) with {@link discover}:
 *
 * ```ts
 * import { discover } from "@skilltest/vitest/vitest";
 * discover("cases");
 * ```
 *
 * For matrices or deterministic mix-in checks, call {@link runSkill} from an
 * ordinary `test()` instead. This module imports `vitest`, so only load it
 * inside a vitest run.
 */
import { type Dirent, readdirSync } from "node:fs";
import { join, relative } from "node:path";
import { expect, test } from "vitest";
import type { RunOptions } from "./runner.js";
import { runSkill } from "./runner.js";
import { describeFailures } from "./schema.js";

/** Filename suffixes a case file must carry to be auto-discovered. */
export const CASE_SUFFIXES = [".skilltest.yaml", ".skilltest.yml"] as const;

/** Register a vitest test that runs `casePath` and asserts every eval passed. */
export function skillTest(name: string, casePath: string, options: RunOptions = {}): void {
  test(name, async () => {
    const report = await runSkill(casePath, options);
    expect(report.passed, describeFailures(report)).toBe(true);
  });
}

/**
 * Recursively find every `*.skilltest.yaml` / `*.skilltest.yml` case under `dir`
 * and register each as a vitest test (named by its path relative to `dir`).
 *
 * vitest only collects its own test modules, so it can't pick up bare YAML the
 * way pytest's collector does. Calling `discover` from a single `*.test.ts` is
 * the closest equivalent: one line gives you pytest-style auto-collection. Cases
 * are sorted for a stable order, and discovery is synchronous so the tests are
 * registered before vitest collects the file.
 */
export function discover(dir = ".", options: RunOptions = {}): void {
  let entries: Dirent<string>[];
  try {
    entries = readdirSync(dir, { recursive: true, withFileTypes: true });
  } catch (err) {
    throw new Error(
      `skilltest discover: cannot read directory \`${dir}\`: ${(err as Error).message}`,
    );
  }
  const cases = entries
    .filter(
      (entry) => entry.isFile() && CASE_SUFFIXES.some((suffix) => entry.name.endsWith(suffix)),
    )
    .map((entry) => join(entry.parentPath, entry.name))
    .sort();
  for (const casePath of cases) {
    skillTest(relative(dir, casePath) || casePath, casePath, options);
  }
}
