/**
 * Errors mirroring the CLI's exit-code contract. A *test failure* (exit 1) is
 * returned as a Report with `passed === false`, not thrown; *bad input* (exit 2)
 * and *provider failure* (exit 3) are thrown because the author must fix them.
 */

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
  constructor(message: string) {
    super(message);
    this.name = "SkilltestProviderError";
  }
}
