/**
 * Errors mirroring the CLI's exit-code contract. A *test failure* (exit 1) is
 * returned as a Report with `passed === false`, not thrown; *bad input* (exit 2)
 * and *provider failure* (exit 3) are thrown because the author must fix them.
 *
 * A provider failure additionally carries a structured classification
 * ({@link SkilltestProviderError.kind}) parsed from the CLI's JSON error output,
 * and is thrown as the **kind-specific subclass** (e.g. {@link
 * SkilltestTimeoutError}) so a handler can `instanceof`-check one category. Every
 * subclass extends {@link SkilltestProviderError}, so a catch on the base still
 * catches them all.
 *
 * The kind→subclass registry is a `Record<ProviderErrorKind, …>`, so `tsc` fails
 * to compile if a kind is added to the Rust enum (which regenerates
 * `ProviderErrorKind`) without a class here — drift is a build error.
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
   * Branch on the subclass ({@link SkilltestTimeoutError}, …) or on this field
   * instead of parsing {@link Error.message}.
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

/** Authentication/authorization failed (missing or rejected credentials). */
export class SkilltestAuthError extends SkilltestProviderError {
  constructor(message: string, options: { context?: string } = {}) {
    super(message, { ...options, kind: "auth" });
    this.name = "SkilltestAuthError";
  }
}

/** The provider rate-limited the call; a backoff-and-retry may succeed. */
export class SkilltestRateLimitError extends SkilltestProviderError {
  constructor(message: string, options: { context?: string } = {}) {
    super(message, { ...options, kind: "rate_limit" });
    this.name = "SkilltestRateLimitError";
  }
}

/** The harness/vendor does not recognize the requested model. */
export class SkilltestModelNotFoundError extends SkilltestProviderError {
  constructor(message: string, options: { context?: string } = {}) {
    super(message, { ...options, kind: "model_not_found" });
    this.name = "SkilltestModelNotFoundError";
  }
}

/** The account's quota or billing limit is exhausted. */
export class SkilltestQuotaError extends SkilltestProviderError {
  constructor(message: string, options: { context?: string } = {}) {
    super(message, { ...options, kind: "quota" });
    this.name = "SkilltestQuotaError";
  }
}

/** A transient server-side overload; retried internally where possible. */
export class SkilltestOverloadedError extends SkilltestProviderError {
  constructor(message: string, options: { context?: string } = {}) {
    super(message, { ...options, kind: "overloaded" });
    this.name = "SkilltestOverloadedError";
  }
}

/** The call exceeded its deadline (harness `--timeout`, curl `--max-time`). */
export class SkilltestTimeoutError extends SkilltestProviderError {
  constructor(message: string, options: { context?: string } = {}) {
    super(message, { ...options, kind: "timeout" });
    this.name = "SkilltestTimeoutError";
  }
}

/** The provider process could not be started (binary missing, not runnable). */
export class SkilltestSpawnError extends SkilltestProviderError {
  constructor(message: string, options: { context?: string } = {}) {
    super(message, { ...options, kind: "spawn" });
    this.name = "SkilltestSpawnError";
  }
}

/** The provider ran but produced output that violated the protocol. */
export class SkilltestProtocolError extends SkilltestProviderError {
  constructor(message: string, options: { context?: string } = {}) {
    super(message, { ...options, kind: "protocol" });
    this.name = "SkilltestProtocolError";
  }
}

type ProviderErrorClass = new (
  message: string,
  options?: { context?: string },
) => SkilltestProviderError;

/**
 * One subclass per concrete kind. Typed as `Record<ProviderErrorKind, …>`, so
 * adding a kind to the Rust enum (which regenerates {@link ProviderErrorKind})
 * breaks compilation here until its subclass is registered — drift is caught by
 * `tsc`, not just at runtime. The `"other"` catch-all maps to the base type.
 */
const PROVIDER_ERROR_CLASSES: Record<ProviderErrorKind, ProviderErrorClass> = {
  auth: SkilltestAuthError,
  rate_limit: SkilltestRateLimitError,
  model_not_found: SkilltestModelNotFoundError,
  quota: SkilltestQuotaError,
  overloaded: SkilltestOverloadedError,
  timeout: SkilltestTimeoutError,
  spawn: SkilltestSpawnError,
  protocol: SkilltestProtocolError,
  other: SkilltestProviderError,
};

/**
 * Build the kind-specific provider error for `kind` (falling back to the base
 * {@link SkilltestProviderError} for `undefined`, `"other"`, or a kind with no
 * registered class).
 */
export function providerErrorFor(
  message: string,
  options: { kind?: ProviderErrorKind; context?: string } = {},
): SkilltestProviderError {
  const { kind, context } = options;
  const cls = kind !== undefined ? PROVIDER_ERROR_CLASSES[kind] : undefined;
  // A concrete subclass fixes its own kind; the base (`other`, an unclassified
  // failure, or a kind with no class) keeps `kind` on the instance.
  if (cls !== undefined && cls !== SkilltestProviderError) {
    return new cls(message, { context });
  }
  return new SkilltestProviderError(message, { kind, context });
}
