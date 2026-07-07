/* eslint-disable */
/**
 * Generated from the golden JSON Schemas in schemas/ by `just gen-contract`.
 * DO NOT MODIFY BY HAND — change the Rust report types and regenerate; the
 * contract drift gate fails while this file is stale.
 */

/**
 * An eval specification, as written in a test case's YAML (or compiled by an
 * SDK's case builders).
 *
 * Newtype variants on purpose: serde cannot `deny_unknown_fields` on an
 * internally tagged enum, but it *does* enforce it on the variant structs, so
 * a typo'd eval field (`expcted:`) is a loud parse error instead of a
 * silently-applied default. The variant titles name the generated SDK model
 * for each union arm, so keep them stable: they are part of the SDK API
 * surface (the input contract, `schemas/case.schema.json`).
 */
export type Eval = BooleanEval | NumericEval | CalledEval | NotCalledEval;
/**
 * How a numeric score is compared to its threshold.
 */
export type Comparator = "gte" | "gt" | "lte" | "lt";
/**
 * A predicate on one tool-input field. Written in YAML either as a bare
 * string (exact equality) or as a map with any of `equals` / `contains` /
 * `pattern` (all given forms must hold).
 *
 * Newtype variants around `deny_unknown_fields` structs, so a typo'd key
 * inside a predicate is a loud parse error, never silently dropped.
 */
export type FieldPredicate = string | FieldPredicateSpec;
/**
 * A `deny` action: block the call with a model-visible message. Written as a
 * bare string (the message) or a map with `message`.
 */
export type DenySpec = string | DenyMessage;
/**
 * A `stub` action: fake a shell call's result by declaring only the output.
 * Written as a bare string (the output) or a map with `output` + `exit_code`.
 * A typo'd key inside the map form is a loud parse error (deny on the inner
 * struct), never a silently-applied default exit code.
 */
export type StubSpec = string | StubOutput;

/**
 * One test case.
 *
 * This type is the source of truth for the **input contract**
 * (`schemas/case.schema.json`): the shape a `--case-json` payload — and the
 * SDKs' generated case models — must have. Serialization skips
 * absent/default fields so the canonical JSON form is minimal; the SDK case
 * builders emit that same form (pinned by the kitchen-sink golden in
 * `tests/fixtures/contract/`).
 */
export interface TestCase {
  /**
   * The evals that decide whether this case passes. Must be non-empty.
   */
  evals: Eval[];
  /**
   * The initial data/prompt handed to the skill as the first user message.
   */
  input: string;
  /**
   * Mock/spy declarations for this case: a declaration with a `stub`/`deny`/
   * `rewrite` action intercepts matching tool calls; one without observes
   * only. `called`/`not_called` evals reference these by `name`.
   */
  mocks?: MockDecl[];
  /**
   * Human-readable name (defaults to the file stem when loaded from a file).
   */
  name?: string;
  /**
   * Path to the skill directory under test, relative to the test-case file.
   */
  skill: string;
  /**
   * Record every tool call through the mock/spy channel even with no
   * `mocks` declared, so code-level consumers (the SDKs' spies) get records.
   * Implied whenever `mocks` is non-empty.
   */
  spy?: boolean;
  /**
   * Present for multi-turn cases; absent for single-turn.
   */
  user?: SimulatedUser | null;
}
/**
 * Assert a plain-English criterion holds (or, with `expected: false`, that
 * it does not).
 */
export interface BooleanEval {
  /**
   * The criterion the judge evaluates against the transcript.
   */
  criterion: string;
  /**
   * What the judge's verdict must equal to pass. Defaults to `true`.
   */
  expected?: boolean;
  /**
   * Optional human label for reports.
   */
  name?: string | null;
  type: "boolean";
}
/**
 * Score a plain-English criterion on a numeric scale and compare it to a
 * threshold.
 */
export interface NumericEval {
  /**
   * How the score is compared to `threshold`. Defaults to `>=`.
   */
  comparator?: Comparator & string;
  /**
   * The criterion the judge scores.
   */
  criterion: string;
  /**
   * Inclusive upper bound of the scale.
   */
  max: number;
  /**
   * Inclusive lower bound of the scale.
   */
  min: number;
  /**
   * Optional human label for reports.
   */
  name?: string | null;
  /**
   * The passing threshold.
   */
  threshold: number;
  type: "numeric";
}
/**
 * Deterministic: assert the referenced mock/spy observed at least one
 * matching call (or exactly `times`). Evaluated against the mock channel's
 * records, not by a judge.
 */
export interface CalledEval {
  /**
   * The `mocks:` declaration (mock or spy) this asserts on, by name.
   */
  mock: string;
  /**
   * Optional human label for reports.
   */
  name?: string | null;
  /**
   * Exact required call count; absent means "at least once".
   */
  times?: number | null;
  type: "called";
  /**
   * Optional per-field input predicates narrowing which calls count.
   */
  where?: {
    [k: string]: FieldPredicate;
  };
}
/**
 * The explicit form of a [`FieldPredicate`]; every given key must hold.
 */
export interface FieldPredicateSpec {
  contains?: string | null;
  equals?: string | null;
  /**
   * Unanchored regex (linear-time; same engine oneharness uses).
   */
  pattern?: string | null;
}
/**
 * Deterministic: assert the referenced mock/spy observed **no** matching
 * call.
 */
export interface NotCalledEval {
  /**
   * The `mocks:` declaration (mock or spy) this asserts on, by name.
   */
  mock: string;
  /**
   * Optional human label for reports.
   */
  name?: string | null;
  type: "not_called";
  /**
   * Optional per-field input predicates narrowing which calls count.
   */
  where?: {
    [k: string]: FieldPredicate;
  };
}
/**
 * One mock or spy declaration — an entry of a test case's `mocks:` block, of
 * the CLI's `--mocks` file, or synthesized by an SDK. Exactly one of `stub` /
 * `deny` / `rewrite` makes it a **mock** (the call is intercepted); none makes
 * it a **spy** (observed only, matched locally against the returned records).
 */
export interface MockDecl {
  /**
   * Block the call; the model reads the message as the tool's feedback.
   */
  deny?: DenySpec | null;
  match: MockMatch;
  /**
   * Name evals (`type: called` / `not_called`) reference this declaration
   * by. Optional; unnamed declarations get a positional fallback
   * (`mock_<i>` / `spy_<i>`) used in reports and error messages.
   */
  name?: string | null;
  /**
   * Substitute raw input fields (a JSON object) — the low-level escape
   * hatch, and the way to mock file reads (rewrite `file_path` to a
   * fixture).
   */
  rewrite?: {
    [k: string]: unknown;
  };
  /**
   * Fake a shell call's result: the real command never runs and the model
   * receives this output as the tool's genuine result.
   */
  stub?: StubSpec | null;
}
/**
 * The explicit form of a [`DenySpec`]: the model-visible message.
 */
export interface DenyMessage {
  message: string;
}
/**
 * What a mock/spy declaration matches on. At least one criterion is required;
 * every given criterion must hold (AND).
 */
export interface MockMatch {
  /**
   * Substring match over the raw hook event JSON (the tool name and its
   * input always serialize into it). Note the haystack is JSON: quotes and
   * backslashes inside tool input appear escaped.
   */
  contains?: string | null;
  /**
   * Per-field predicates on the tool's input: the key is the argument name
   * (`command`, `file_path`, …), and every listed field must match (a field
   * absent from the call fails the matcher). Non-string fields are compared
   * against their compact JSON form.
   */
  input?: {
    [k: string]: FieldPredicate;
  };
  /**
   * Unanchored regex over the same haystack as `contains`, for non-exact
   * needles like `git push( --force)?`. Linear-time engine (no lookarounds).
   */
  pattern?: string | null;
  /**
   * Case-insensitive exact match on the tool name. Tool names are
   * per-harness (`Bash` on claude-code, `bash` on opencode/crush), so
   * cross-harness matchers usually prefer `contains`/`pattern`/`input`.
   */
  tool?: string | null;
}
/**
 * The explicit form of a [`StubSpec`]: the canned output plus an exit code.
 */
export interface StubOutput {
  /**
   * Non-zero fakes a failing command. Default 0.
   */
  exit_code?: number;
  output: string;
}
/**
 * The simulated-user block that turns a single-turn case into a multi-turn one.
 * When present, after each assistant turn the runner asks the provider to play
 * the user (guided by `persona`) until `done_when` holds or `max_turns` is hit.
 */
export interface SimulatedUser {
  /**
   * A plain-English condition; when the judge decides it holds, the
   * conversation ends. Optional — without it the run ends at `max_turns` or
   * when the skill reports itself done.
   */
  done_when?: string | null;
  /**
   * Per-case override of the global assistant-turn cap.
   */
  max_turns?: number | null;
  /**
   * Instructions describing how the simulated user should behave.
   */
  persona: string;
}
