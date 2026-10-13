/* eslint-disable */
/**
 * Generated from the golden JSON Schemas in schemas/ by `just gen-contract`.
 * DO NOT MODIFY BY HAND — change the Rust report types and regenerate; the
 * contract drift gate fails while this file is stale.
 */

/**
 * How evals and the simulated user are judged, independent of the provider that
 * runs the skill. Absent (the default) means the run's provider judges too
 * (e.g. the oneharness `judge_harness`).
 */
export type JudgeConfigOverrides = {
  /**
   * Environment variable holding the API key. Defaults to `ANTHROPIC_API_KEY`
   * or `OPENAI_API_KEY` by vendor. The key is read at run time and never
   * stored in config.
   */
  api_key_env?: string | null;
  /**
   * Override the API endpoint (e.g. a proxy or an OpenAI-compatible gateway).
   * Defaults to the vendor's standard endpoint.
   */
  base_url?: string | null;
  /**
   * The `curl` binary (resolved on `PATH`).
   */
  curl_bin?: string;
  kind?: "api";
  /**
   * Constrain the judge's verdict to the `{value, reason}` JSON schema via the
   * vendor's structured-outputs feature (Anthropic `output_config.format`,
   * OpenAI `response_format: json_schema`). On by default — it removes a class
   * of judge-parse fragility. Turn it off for a model/endpoint that doesn't
   * support structured outputs (the tolerant `{…}` extraction still applies).
   */
  strict_json?: boolean;
  /**
   * Per-call timeout in seconds, passed to `curl --max-time`.
   */
  timeout_secs?: number;
  /**
   * Which vendor's API to call.
   */
  vendor?: ApiVendorOverrides;
};
/**
 * Which model vendor's API the direct-API judge talks to.
 */
export type ApiVendorOverrides = "anthropic" | "openai";
/**
 * A `deny` action: block the call with a model-visible message. Written as a
 * bare string (the message) or a map with `message`.
 */
export type DenySpec = string | DenyMessage;
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
 * A `stub` action: fake a shell call's result by declaring only the output.
 * Written as a bare string (the output), a map with `output` + `exit_code`,
 * or a non-empty sequence of those two forms: the *n*-th call the mock
 * intercepts within one skill run gets the *n*-th item, and every call after
 * the last item gets the last item again. A typo'd key inside the map form is
 * a loud parse error (deny on the inner struct), never a silently-applied
 * default exit code.
 */
export type StubSpec = string | StubOutput | [StubResponse, ...StubResponse[]];
/**
 * One canned response: a bare string (the output, exit code 0) or a map with
 * `output` + `exit_code`. Both a single `stub` and each item of a `stub`
 * sequence take this shape.
 */
export type StubResponse = string | StubOutput;
/**
 * Which provider backs a run.
 */
export type ProviderConfigOverrides =
  | {
      /**
       * The `oneharness` binary. When unset, defaults to `$SKILLTEST_ONEHARNESS_BIN`
       * (the path the SDKs point at their bundled `oneharness-cli`), else
       * `oneharness` resolved on `PATH`.
       */
      bin?: string;
      /**
       * Record each skill run to oneharness's run history (`oneharness run
       * --history`), so a past run can be reviewed later with `oneharness
       * history`. On by default. The judge and simulated-user calls are never
       * recorded — only the skill under test.
       */
      history?: boolean;
      /**
       * Directory the shared run history is written to (passed as `oneharness run
       * --history-dir`). Defaults to a single centralized location reused across
       * every skilltest invocation (see
       * [`crate::provider::default_history_dir`]), so past runs accumulate in one
       * reviewable place instead of scattering per project.
       */
      history_dir?: string | null;
      /**
       * Full oneharness invocation settings for the judge. Omitted settings belong to oneharness.
       */
      judge?: OneharnessRoleConfigOverrides | null;
      /**
       * The harness used for evals and the simulated user (kept independent of the
       * harness under test, so the evaluator does not vary with the matrix).
       */
      judge_harness?: string;
      kind?: "oneharness";
      /**
       * Full simulated-user invocation settings; absent inherits `judge`.
       */
      simulated_user?: OneharnessRoleConfigOverrides | null;
      /**
       * Per-call timeout passed through to `oneharness run --timeout`.
       */
      timeout_secs?: number;
    }
  | {
      /**
       * The provider command as an argv vector.
       */
      command?: string[];
      kind?: "command";
    };

/**
 * The full configuration for a run.
 */
export interface ConfigOverrides {
  /**
   * Optional judge backend that overrides how evals and the simulated user are
   * scored, independent of the skill-running provider. When `None`, the
   * provider judges (e.g. the oneharness `judge_harness`).
   */
  judge?: JudgeConfigOverrides | null;
  /**
   * Model used for natural-language evals and the simulated user. Falls back
   * to the first entry of `models` when empty.
   */
  judge_model?: string;
  /**
   * Default cap on assistant turns for multi-turn cases. A case may lower it.
   */
  max_turns?: number;
  /**
   * Shared mock/spy declarations, prepended to every case's own `mocks`
   * (first match wins, so these shadow the case's). Populated by the CLI's
   * `--mocks <file>` — how the SDKs deliver code-level mocks — or written
   * here directly for suite-wide rules.
   */
  mocks?: MockDecl[];
  /**
   * Models a case runs on (must be valid for the chosen harness, e.g.
   * `sonnet`/`haiku` for `claude-code`).
   */
  models?: string[];
  /**
   * Harness platforms a case runs on (e.g. `claude-code`, `codex`).
   */
  platforms?: string[];
  /**
   * The provider that executes skills and evals.
   */
  provider?: ProviderConfigOverrides;
  /**
   * Force the mock/spy observation channel on for every run, even for cases
   * that declare no `mocks` (the CLI's `--spy`; how SDK spies get records).
   */
  spy?: boolean;
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
 * Literal argv/config pass-through for one buffered evaluator invocation.
 */
export interface OneharnessRoleConfigOverrides {
  /**
   * Raw oneharness run argv tokens; harness-tail tokens after `--` stay literal.
   */
  args?: string[];
  /**
   * oneharness config path, resolved from the invocation working directory.
   */
  config?: string | null;
}
