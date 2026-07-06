/* eslint-disable */
/**
 * Generated from the golden JSON Schemas in schemas/ by `just gen-contract`.
 * DO NOT MODIFY BY HAND — change the Rust report types and regenerate; the
 * contract drift gate fails while this file is stale.
 */

/**
 * The kind-specific detail of an eval outcome, for reporting.
 *
 * The variant titles name the generated SDK model for each union arm, so keep
 * them stable: they are part of the SDK API surface.
 */
export type EvalDetail = BooleanDetail | NumericDetail;
/**
 * How a numeric score is compared to its threshold.
 */
export type Comparator = "gte" | "gt" | "lte" | "lt";
/**
 * Who produced a message.
 */
export type Role = "user" | "assistant" | "system";

/**
 * The top-level report for a `skilltest run` invocation.
 */
export interface Report {
  /**
   * True iff every run passed.
   */
  passed: boolean;
  /**
   * Every individual run.
   */
  runs: CaseRun[];
  /**
   * Aggregate counts.
   */
  summary: Summary;
}
/**
 * The result of running one test case on one (platform, model) pair.
 */
export interface CaseRun {
  /**
   * The test case name.
   */
  case: string;
  /**
   * Per-eval outcomes, in declaration order.
   */
  evals: EvalOutcome[];
  /**
   * The model this run used.
   */
  model: string;
  /**
   * True iff every eval in this run passed.
   */
  passed: boolean;
  /**
   * The harness platform this run used.
   */
  platform: string;
  /**
   * Absolute-ish path to the skill that was exercised.
   */
  skill: string;
  /**
   * The full conversation, for debugging and deterministic mix-in checks.
   */
  transcript: Transcript;
  /**
   * Number of assistant turns produced.
   */
  turns: number;
  /**
   * Aggregated token/cost usage across every provider call in this run
   * (skill turns + simulated-user turns + judge calls). Omitted when no
   * usage was reported (e.g. the fake provider or a harness that doesn't
   * surface usage).
   */
  usage?: Usage | null;
}
/**
 * The result of running one eval against a transcript.
 */
export interface EvalOutcome {
  /**
   * Kind-specific verdict detail.
   */
  detail: EvalDetail;
  /**
   * The eval's label (name or criterion).
   */
  label: string;
  /**
   * Whether the eval passed.
   */
  passed: boolean;
  /**
   * The judge's stated reason.
   */
  reason: string;
}
export interface BooleanDetail {
  expected: boolean;
  kind: "boolean";
  value: boolean;
}
export interface NumericDetail {
  comparator: Comparator;
  kind: "numeric";
  threshold: number;
  value: number;
}
/**
 * An ordered list of messages. Thin wrapper so the type reads clearly at call
 * sites and so we can grow conversation-level helpers without churn.
 */
export interface Transcript {
  messages: Message[];
}
/**
 * A single turn in the conversation.
 */
export interface Message {
  content: string;
  /**
   * The normalized tool events the skill took producing this turn (assistant
   * turns only, and only when the harness exposed a tool transcript via
   * oneharness `--events`). Empty otherwise. Surfaced for post-hoc analysis
   * and streamed live for short-circuiting.
   */
  events?: ToolEvent[];
  role: Role;
}
/**
 * One normalized tool-call / action event the skill took during a turn, lifted
 * from oneharness's `events` array (its `--events` output). Harness-agnostic, so
 * a consumer can inspect *what the skill did* — shell commands, file edits, tool
 * uses — across any harness, not just the final text. Mirrors the oneharness
 * action-event shape; `input` is the structured, tool-shaped args so a consumer
 * can match on the command string or file path without re-parsing.
 *
 * `input` is a free-form JSON value, so `Message`/`Transcript` are `PartialEq`
 * but not `Eq`.
 */
export interface ToolEvent {
  /**
   * Position within the turn, so ordering ("did X before Y") is expressible.
   */
  index?: number;
  /**
   * Structured tool arguments (the command, the file path); `null` when none.
   */
  input?: {
    [k: string]: unknown;
  };
  /**
   * `tool_call` (the skill invoked a tool) or `tool_result` (the observation).
   */
  kind: string;
  /**
   * Normalized tool name where knowable (e.g. `bash`, `edit_file`); `null` for
   * a `tool_result` or when the harness did not name it.
   */
  name?: string | null;
  /**
   * The result/observation text, when the transcript exposed it.
   */
  output?: string | null;
}
/**
 * Token / cost usage for one provider call.
 *
 * Each field is independently optional because not every harness reports every
 * signal (cost is commonly absent on subscription auth; some harnesses report
 * no usage at all). The whole struct is `Option<Usage>` on a turn — `None`
 * means "no signal," not "zero."
 */
export interface Usage {
  cost_usd?: number | null;
  input_tokens?: number | null;
  output_tokens?: number | null;
}
/**
 * Aggregate pass/fail counts for a report.
 */
export interface Summary {
  /**
   * Distinct test cases represented.
   */
  cases: number;
  /**
   * Runs that failed.
   */
  failed: number;
  /**
   * Runs that passed.
   */
  passed: number;
  /**
   * Total (case × platform × model) runs.
   */
  runs: number;
  /**
   * Aggregated token/cost usage across every run in the report. Omitted
   * when no run reported usage.
   */
  usage?: Usage | null;
}
