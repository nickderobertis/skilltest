/* eslint-disable */
/**
 * Generated from the golden JSON Schemas in schemas/ by `just gen-contract`.
 * DO NOT MODIFY BY HAND — change the Rust report types and regenerate; the
 * contract drift gate fails while this file is stale.
 */

/**
 * Which class of failure a [`ReportError`] describes. Mirrors the process exit
 * code so a JSON consumer gets the same coarse classification as a shell script
 * branching on `$?`.
 */
export type ErrorCode = "usage" | "provider";
/**
 * Structured classification of a provider failure.
 *
 * This is the machine-readable category SDK/plugin consumers branch on instead
 * of matching substrings in the human message (the reason this type exists: a
 * timeout is [`ProviderErrorKind::Timeout`], not `message.contains("timed
 * out")`). It is part of the JSON error contract — it serializes as a
 * snake_case string and is re-exported through every SDK's generated models, so
 * the vocabulary here is the vocabulary consumers see.
 *
 * The set is closed on purpose so the generated SDK types are exhaustive
 * unions; a failure skilltest cannot map to a specific category (e.g. a
 * provider or oneharness failure label added upstream that this version does
 * not recognize) becomes [`ProviderErrorKind::Other`] rather than a new,
 * unhandled string.
 */
export type ProviderErrorKind =
  | "auth"
  | "rate_limit"
  | "model_not_found"
  | "quota"
  | "overloaded"
  | "timeout"
  | "spawn"
  | "protocol"
  | "other";

/**
 * A structured error, emitted as the `--format json` / `json-stream` output
 * when a `skilltest run` cannot produce a [`Report`].
 *
 * This is the machine-readable counterpart to the human hint the CLI prints on
 * stderr: it rides on stdout so SDK/plugin consumers get the [`ProviderErrorKind`]
 * (and the `code`/`context`) for targeted handling — retry on
 * [`ProviderErrorKind::Timeout`], fail fast on [`ProviderErrorKind::Auth`] —
 * instead of matching substrings in the message. For `json` the object is
 * emitted bare; for `json-stream` it is the terminal
 * `{"type":"error","error":{…}}` line.
 */
export interface ReportError {
  /**
   * The coarse failure class, matching the process exit code.
   */
  code: ErrorCode;
  /**
   * The provider context the failure came from (e.g. `oneharness:claude-code`,
   * `api-judge`). Absent for usage errors.
   */
  context?: string | null;
  /**
   * The structured provider-failure category, when skilltest could classify
   * it. Absent for usage errors and for unclassified provider failures.
   */
  kind?: ProviderErrorKind | null;
  /**
   * A human-readable description of what went wrong (the same text printed on
   * stderr, minus the suggested-action hint).
   */
  message: string;
}
