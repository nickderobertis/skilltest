/**
 * `@skill-test/sdk` — the TypeScript SDK for the `skilltest` CLI.
 *
 * A thin, typed wrapper around the CLI and nothing else: run test cases,
 * validate skills, and get back objects typed by declarations **generated from
 * the CLI's own JSON Schemas** (`just gen-contract`), so the types cannot
 * drift from the binary. Test frameworks build on this — `@skill-test/vitest`
 * adds the vitest helpers on top.
 *
 * Define the whole case in code (the recommended form):
 *
 * ```ts
 * import { runSkill, testCase, boolean, describeFailures } from "@skill-test/sdk";
 *
 * const report = await runSkill(
 *   testCase({
 *     skill: "skills/greeter",
 *     input: "Greet Dr. Smith, who has an appointment today.",
 *     evals: [boolean("the reply greets Dr. Smith by name")],
 *   }),
 * );
 * if (!report.passed) throw new Error(describeFailures(report));
 * ```
 *
 * `runSkill` also takes a path to an existing test-case YAML file (or a
 * directory of them), and the vitest plugin's `discover` collects a tree of
 * `*.skilltest.yaml` files.
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
  type BooleanEval,
  type CalledEval,
  type CaseEval,
  type ComparatorInput,
  type Eval,
  type MockRefEval,
  type NotCalledEval,
  type NumericEval,
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
  SkilltestAuthError,
  SkilltestError,
  SkilltestModelNotFoundError,
  SkilltestOverloadedError,
  SkilltestProtocolError,
  SkilltestProviderError,
  SkilltestQuotaError,
  SkilltestRateLimitError,
  SkilltestSpawnError,
  SkilltestTimeoutError,
  SkilltestUsageError,
} from "./errors.js";
export type { ErrorCode, ProviderErrorKind, ReportError } from "./generated/error.js";
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
