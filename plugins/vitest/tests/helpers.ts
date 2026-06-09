/**
 * Shared test setup: point the plugin at the locally built Rust binaries.
 * Importing this module sets `SKILLTEST_BIN`/`SKILLTEST_PROVIDER` defaults, so
 * import it before invoking the runner.
 */
import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
// plugins/vitest/tests -> repo root
export const REPO_ROOT = join(here, "..", "..", "..");
const TARGET = join(REPO_ROOT, "target", "debug");

export const SKILLTEST_BIN = join(TARGET, "skilltest");
export const FAKE_PROVIDER = join(TARGET, "skilltest-fake-provider");
export const FIXTURES = join(REPO_ROOT, "tests", "fixtures");

export function caseFile(name: string): string {
  return join(FIXTURES, "cases", name);
}

export function skillDir(name: string): string {
  return join(FIXTURES, "skills", name);
}

process.env.SKILLTEST_BIN ??= SKILLTEST_BIN;
process.env.SKILLTEST_PROVIDER ??= FAKE_PROVIDER;

export function requireBinaries(): void {
  for (const path of [SKILLTEST_BIN, FAKE_PROVIDER]) {
    if (!existsSync(path)) {
      throw new Error(
        `built binary not found: ${path}. Run \`just bootstrap\` (cargo build) first.`,
      );
    }
  }
}
