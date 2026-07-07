/**
 * `@skill-test/sdk` — the TypeScript SDK for the `skilltest` CLI.
 *
 * A thin, typed wrapper around the CLI and nothing else: run test cases,
 * validate skills, and get back objects typed by declarations **generated from
 * the CLI's own JSON Schemas** (`just gen-contract`), so the types cannot
 * drift from the binary. Test frameworks build on this — `@skill-test/vitest`
 * adds the vitest helpers on top.
 *
 * ```ts
 * import { runSkill, assistantText, describeFailures } from "@skill-test/sdk";
 *
 * const report = await runSkill("cases/greet.yaml");
 * if (!report.passed) throw new Error(describeFailures(report));
 * ```
 */
export { runSkill, validateSkill, ENV_BIN, ENV_PROVIDER, type RunOptions } from "./runner.js";
export { streamSkill, type SkillStream, type StreamEvent } from "./stream.js";
export {
  testCase,
  boolean,
  numeric,
  called,
  notCalled,
  user,
  type Eval,
  type SimulatedUser,
  type TestCaseInput,
} from "./case.js";
export {
  ToolMock,
  ToolSpy,
  anything,
  contains,
  deny,
  matching,
  rewrite,
  spy,
  stub,
  type Criterion,
  type MatchOptions,
  type Matcher,
  type ToolCall,
  type WhereCriteria,
} from "./mock.js";
export {
  SkilltestError,
  SkilltestProviderError,
  SkilltestUsageError,
} from "./errors.js";
export { assistantText, describeFailures, toolCalls } from "./helpers.js";
export type {
  BooleanDetail,
  CallsDetail,
  CaseRun,
  Comparator,
  EvalDetail,
  EvalOutcome,
  Message,
  MockCall,
  NumericDetail,
  Report,
  Role,
  Summary,
  ToolEvent,
  Transcript,
  Usage,
} from "./generated/report.js";
export type { ValidationFinding, ValidationReport } from "./generated/validation.js";
