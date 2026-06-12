/**
 * `@skill-test/vitest` — run AI-skill tests and natural-language evals in vitest.
 *
 * The vitest integration on top of `@skill-test/sdk`, whose API is re-exported
 * here so a vitest suite needs only this one dependency:
 *
 * ```ts
 * import { skillTest, discover, runSkill, assistantText } from "@skill-test/vitest";
 *
 * skillTest("greeter names the patient", "cases/greet.yaml");
 * // or auto-discover a tree of *.skilltest.yaml cases:
 * discover("cases");
 *
 * // For matrices or deterministic mix-in checks, use the SDK API directly:
 * test("greeter", async () => {
 *   const report = await runSkill("cases/greet.yaml");
 *   expect(report.passed).toBe(true);
 *   expect(assistantText(report.runs[0]!.transcript)).toContain("Dr. Smith");
 * });
 * ```
 *
 * This module (via the helpers) imports `vitest`, so only load it inside a
 * vitest run. `@skill-test/vitest/vitest` remains as an alias for the helpers.
 */
export { skillTest, discover, CASE_SUFFIXES } from "./vitest.js";
export * from "@skill-test/sdk";
