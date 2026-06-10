/**
 * `@skilltest/vitest` — run AI-skill tests and natural-language evals in vitest.
 *
 * ```ts
 * import { runSkill, assistantText } from "@skilltest/vitest";
 *
 * test("greeter", async () => {
 *   const report = await runSkill("cases/greet.yaml");
 *   expect(report.passed).toBe(true);
 *   expect(assistantText(report.runs[0]!.transcript)).toContain("Dr. Smith");
 * });
 * ```
 *
 * The one-line vitest helper lives at `@skilltest/vitest/vitest`.
 */
export { runSkill, validateSkill, ENV_BIN, ENV_PROVIDER, type RunOptions } from "./runner.js";
export {
  SkilltestError,
  SkilltestProviderError,
  SkilltestUsageError,
} from "./errors.js";
export {
  assistantText,
  describeFailures,
  type CaseRun,
  type EvalDetail,
  type EvalOutcome,
  type Message,
  type Report,
  type Summary,
  type Transcript,
  type Usage,
  type ValidationFinding,
  type ValidationReport,
} from "./schema.js";
