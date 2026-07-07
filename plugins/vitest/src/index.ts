/**
 * `@skill-test/vitest` — run AI-skill tests and natural-language evals in vitest.
 *
 * The vitest integration on top of `@skill-test/sdk`, whose API is re-exported
 * here so a vitest suite needs only this one dependency. Define the whole case
 * in code (the recommended form) and register it in one line:
 *
 * ```ts
 * import { skillTest, testCase, boolean } from "@skill-test/vitest";
 *
 * skillTest(
 *   "greeter names the patient",
 *   testCase({
 *     skill: "skills/greeter",
 *     input: "Greet Dr. Smith.",
 *     evals: [boolean("the reply greets Dr. Smith by name")],
 *   }),
 * );
 * ```
 *
 * Existing YAML cases stay first-class: `skillTest("greeter", "cases/greet.yaml")`
 * runs a file, and `discover("cases")` auto-collects a whole tree of
 * `*.skilltest.yaml` cases. For matrices or deterministic mix-in checks, call
 * the SDK's `runSkill` from an ordinary `test()`.
 *
 * This module (via the helpers) imports `vitest`, so only load it inside a
 * vitest run. `@skill-test/vitest/vitest` remains as an alias for the helpers.
 */
export { skillTest, discover, CASE_SUFFIXES } from "./vitest.js";
export * from "@skill-test/sdk";
