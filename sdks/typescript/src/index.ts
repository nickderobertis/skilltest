/**
 * `@skilltest/sdk` — the TypeScript SDK for the `skilltest` CLI.
 *
 * A thin, typed wrapper around the CLI and nothing else: run test cases,
 * validate skills, and get back Zod-parsed objects mirroring the stable
 * `--format json` contract. Test frameworks build on this — `@skilltest/vitest`
 * adds the vitest helpers on top.
 *
 * ```ts
 * import { runSkill, assistantText } from "@skilltest/sdk";
 *
 * const report = await runSkill("cases/greet.yaml");
 * if (!report.passed) throw new Error(describeFailures(report));
 * ```
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
  BooleanDetailSchema,
  CaseRunSchema,
  ComparatorSchema,
  EvalDetailSchema,
  EvalOutcomeSchema,
  MessageSchema,
  NumericDetailSchema,
  ReportSchema,
  SummarySchema,
  TranscriptSchema,
  UsageSchema,
  ValidationFindingSchema,
  ValidationReportSchema,
  type CaseRun,
  type Comparator,
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
