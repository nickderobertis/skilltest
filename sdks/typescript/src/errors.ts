/**
 * Errors mirroring the CLI's exit-code contract. A *test failure* (exit 1) is
 * returned as a Report with `passed === false`, not thrown; *bad input* (exit 2)
 * and *provider failure* (exit 3) are thrown because the author must fix them.
 *
 * A provider failure additionally carries a structured classification
 * ({@link SkilltestProviderError.kind}) parsed from the CLI's JSON error output,
 * so consumers can branch on the category (retry a `"timeout"`, fail fast on
 * `"auth"`) instead of matching substrings in the message.
 */
import type { ProviderErrorKind } from "./generated/error.js";

export class SkilltestError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "SkilltestError";
  }
}

export class SkilltestUsageError extends SkilltestError {
  constructor(message: string) {
    super(message);
    this.name = "SkilltestUsageError";
  }
}

export class SkilltestProviderError extends SkilltestError {
  /**
   * The structured failure category (e.g. `"timeout"`, `"auth"`) when skilltest
   * could classify it; `undefined` for an older CLI or an unclassified failure.
   * Branch on this instead of parsing {@link Error.message}.
   */
  readonly kind: ProviderErrorKind | undefined;
  /** The provider the failure came from (e.g. `"oneharness:claude-code"`). */
  readonly context: string | undefined;

  constructor(message: string, options: { kind?: ProviderErrorKind; context?: string } = {}) {
    super(message);
    this.name = "SkilltestProviderError";
    this.kind = options.kind;
    this.context = options.context;
  }
}
