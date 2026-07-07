/**
 * Run the `skilltest` CLI as a subprocess and parse its JSON contract.
 *
 * This is the code-level API: call {@link runSkill}, get a typed
 * {@link Report}, assert on `report.passed`, and mix in deterministic checks
 * against the transcript.
 */
import { spawn } from "node:child_process";
import {
  constants,
  accessSync,
  chmodSync,
  existsSync,
  mkdtempSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { type TestCaseInput, compileCase, isTestCaseInput } from "./case.js";
import {
  SkilltestError,
  SkilltestProviderError,
  SkilltestUsageError,
  providerErrorFor,
} from "./errors.js";
import type { ReportError } from "./generated/error.js";
import type { Report } from "./generated/report.js";
import type { ValidationReport } from "./generated/validation.js";
import { type ToolSpy, bindMocks, compileDecls } from "./mock.js";

/** Environment variables supplying defaults for the binary and provider. */
export const ENV_BIN = "SKILLTEST_BIN";
export const ENV_PROVIDER = "SKILLTEST_PROVIDER";

export interface RunOptions {
  /** Path to the `skilltest` binary (default: `$SKILLTEST_BIN` or `skilltest`). */
  bin?: string;
  /** Provider command (default: `$SKILLTEST_PROVIDER`). A string or argv array. */
  provider?: string | string[];
  /** Harness platforms to run on (overrides config). */
  platforms?: string[];
  /** Models to run on (overrides config). */
  models?: string[];
  /** Model used for evals and the simulated user. */
  judgeModel?: string;
  /** Cap on assistant turns for multi-turn cases. */
  maxTurns?: number;
  /** Path to a config file. */
  config?: string;
  /** Working directory for the subprocess. */
  cwd?: string;
  /**
   * Mock/spy objects ({@link import("./mock.js").spy | spy} /
   * {@link import("./mock.js").stub | stub} /
   * {@link import("./mock.js").deny | deny} /
   * {@link import("./mock.js").rewrite | rewrite}): mocks compile into the
   * run's hook-side ruleset (prepended to the case's own `mocks:` block, so
   * the test-local rule wins), and after the run every object is bound with
   * the calls it matched — assert on it directly. Each run re-binds fresh.
   */
  mocks?: ToolSpy[];
}

interface Captured {
  status: number | null;
  stdout: string;
  stderr: string;
}

const require = createRequire(import.meta.url);

/**
 * The optional platform package that carries the prebuilt binary for this host,
 * e.g. `@skill-test/cli-linux-x64`. One is published per supported target and
 * declared in `optionalDependencies`; the package manager installs only the one
 * matching the host's `os`/`cpu`, so this name resolves to an installed package
 * on exactly the supported platforms.
 */
export function platformPackage(): string {
  return `@skill-test/cli-${process.platform}-${process.arch}`;
}

/**
 * Absolute path to the binary bundled in the matching platform package, or
 * `undefined` when none is installed — a source/dev checkout (the package links
 * but ships no binary), or a platform with no published package. Callers fall
 * back to `$SKILLTEST_BIN`/`PATH`.
 */
export function bundledBin(): string | undefined {
  try {
    const pkgJson = require.resolve(`${platformPackage()}/package.json`);
    const exe = process.platform === "win32" ? "skilltest.exe" : "skilltest";
    const bin = join(dirname(pkgJson), "bin", exe);
    if (!existsSync(bin)) return undefined;
    ensureExecutable(bin);
    return bin;
  } catch {
    return undefined;
  }
}

// Some packers (pnpm pack) drop the executable bit; restore it best-effort. The
// platform packages publish via `npm` (which preserves +x), so this only matters
// as a fallback — and a read-only install keeps the packed mode regardless.
function ensureExecutable(bin: string): void {
  try {
    accessSync(bin, constants.X_OK);
  } catch {
    try {
      chmodSync(bin, 0o755);
    } catch {
      // best effort; if it is not executable and not chmod-able, the spawn fails
      // with a clear EACCES that points at $SKILLTEST_BIN.
    }
  }
}

/**
 * Resolve the binary to run, most explicit first: an explicit `bin`, then
 * `$SKILLTEST_BIN`, then the bundled platform binary, then `skilltest` on PATH.
 */
export function resolveBin(bin: string | undefined): string {
  return bin ?? process.env[ENV_BIN] ?? bundledBin() ?? "skilltest";
}

export function resolveProvider(provider: string | string[] | undefined): string | undefined {
  const value = provider ?? process.env[ENV_PROVIDER];
  if (value === undefined) return undefined;
  return Array.isArray(value) ? value.join(" ") : value;
}

/**
 * The `skilltest run` fragments a `case` becomes (shared with the streaming
 * API): a YAML file/directory rides as a positional path; a code-defined
 * {@link TestCaseInput} is written to a temp JSON file passed as `--case-json`.
 * Call `cleanup()` once the run has finished with the file.
 */
export function caseRunArgs(caseInput: string | TestCaseInput): {
  args: string[];
  cleanup: () => void;
} {
  if (typeof caseInput === "string") return { args: [caseInput], cleanup: () => {} };
  const dir = mkdtempSync(join(tmpdir(), "skilltest-case-"));
  const file = join(dir, "case.json");
  writeFileSync(file, JSON.stringify([compileCase(caseInput)]));
  return {
    args: ["--case-json", file],
    cleanup: () => {
      try {
        rmSync(dir, { recursive: true, force: true });
      } catch {
        // best-effort temp cleanup
      }
    },
  };
}

/**
 * Build the `skilltest run` args for output format `format` (`json` for the
 * buffered API, `json-stream` for the streaming API). `caseArgs` are the
 * fragments identifying the case(s) — a positional path or `--case-json <file>`
 * (see {@link caseRunArgs}). Shared by {@link runSkill} and the streaming API.
 */
export function buildRunArgs(
  caseArgs: string[],
  options: RunOptions,
  format: string,
  mockArgs: string[] = [],
): string[] {
  const args: string[] = [];
  if (options.config) args.push("--config", options.config);
  args.push("run", ...caseArgs, "--format", format);

  const provider = resolveProvider(options.provider);
  if (provider !== undefined) args.push("--provider", provider);
  for (const platform of options.platforms ?? []) args.push("--platform", platform);
  for (const model of options.models ?? []) args.push("--model", model);
  if (options.judgeModel) args.push("--judge-model", options.judgeModel);
  if (options.maxTurns !== undefined) args.push("--max-turns", String(options.maxTurns));
  args.push(...mockArgs);
  return args;
}

/**
 * The CLI flags a `mocks` option turns into (shared with the streaming API):
 * `--spy` so the observation channel is on for spies, plus `--mocks <file>`
 * carrying the compiled mock declarations in a temp dir. Call `cleanup()` once
 * the run has finished with the file.
 */
export function mockRunArgs(mocks: readonly ToolSpy[] | undefined): {
  args: string[];
  cleanup: () => void;
} {
  if (!mocks || mocks.length === 0) return { args: [], cleanup: () => {} };
  const args = ["--spy"];
  const decls = compileDecls(mocks);
  if (decls.length === 0) return { args, cleanup: () => {} };
  const dir = mkdtempSync(join(tmpdir(), "skilltest-mocks-"));
  const file = join(dir, "mocks.json");
  writeFileSync(file, JSON.stringify(decls));
  return {
    args: [...args, "--mocks", file],
    cleanup: () => {
      try {
        rmSync(dir, { recursive: true, force: true });
      } catch {
        // best-effort temp cleanup
      }
    },
  };
}

/**
 * The structured error the CLI emits on stdout for a `--format json` failure, or
 * `undefined` when stdout is not that envelope (an older binary that printed
 * nothing, or a non-JSON line) — callers fall back to the stderr text. Shared by
 * the buffered and streaming APIs. Lightweight-guarded rather than validated:
 * an error envelope has a string `code`/`message` a Report never has.
 */
export function parseReportError(stdout: string): ReportError | undefined {
  const text = stdout.trim();
  if (!text) return undefined;
  try {
    const obj = JSON.parse(text);
    if (
      obj &&
      (obj.code === "usage" || obj.code === "provider") &&
      typeof obj.message === "string"
    ) {
      return obj as ReportError;
    }
  } catch {
    // not JSON — fall back to the stderr text
  }
  return undefined;
}

/**
 * Map a skilltest exit code to a thrown error (shared by the buffered and
 * streaming APIs). Codes 0/1 produce a report and never throw. `structured`,
 * when the CLI emitted the JSON error envelope, carries the classified
 * `kind`/`context` onto a {@link SkilltestProviderError}.
 */
export function raiseForCode(code: number | null, detail: string, structured?: ReportError): void {
  if (code === 0 || code === 1 || code === null) return;
  const message = structured?.message || detail;
  if (code === 2) throw new SkilltestUsageError(message);
  if (code === 3) {
    // The kind-specific subclass (SkilltestTimeoutError, …) so a handler can
    // `instanceof`-check one category; falls back to the base for other/none.
    throw providerErrorFor(message, {
      kind: structured?.kind ?? undefined,
      context: structured?.context ?? undefined,
    });
  }
  throw new SkilltestError(`skilltest exited ${code}: ${message}`);
}

function capture(bin: string, args: string[], cwd: string | undefined): Promise<Captured> {
  return new Promise((resolve, reject) => {
    const child = spawn(bin, args, { cwd });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk: Buffer) => {
      stdout += chunk.toString();
    });
    child.stderr.on("data", (chunk: Buffer) => {
      stderr += chunk.toString();
    });
    child.on("error", (err) =>
      reject(
        new SkilltestProviderError(
          `could not run skilltest binary \`${bin}\`: ${err.message}. Set ${ENV_BIN} or pass bin.`,
        ),
      ),
    );
    child.on("close", (status) => resolve({ status, stdout, stderr }));
  });
}

// Exit codes that still produce a JSON report (0 = all passed, 1 = some failed).
function raiseForStatus(result: Captured): void {
  if (result.status === 0 || result.status === 1) return;
  const structured = parseReportError(result.stdout);
  const detail = result.stderr.trim() || result.stdout.trim();
  raiseForCode(result.status, detail, structured);
}

// The cast is sound by construction: the SDK's types are generated from the
// CLI's own JSON Schemas and the contract drift gate (`just gen-contract
// --check` in CI) fails when they diverge, so the shape is not re-validated
// here at runtime.
function parse<T>(stdout: string): T {
  try {
    return JSON.parse(stdout) as T;
  } catch (err) {
    throw new SkilltestError(`skilltest did not emit JSON: ${(err as Error).message}`);
  }
}

/**
 * Run one or more test cases and return the parsed {@link Report}. `caseInput`
 * is a code-defined {@link TestCaseInput} (the recommended form) or a path to a
 * YAML file/directory. A failing eval is reported in `report.passed`, not
 * thrown; only bad input ({@link SkilltestUsageError}) and provider failures
 * ({@link SkilltestProviderError}) throw.
 */
export async function runSkill(
  caseInput: string | TestCaseInput,
  options: RunOptions = {},
): Promise<Report> {
  const caseArgs = caseRunArgs(caseInput);
  const mocks = mockRunArgs(options.mocks);
  let result: Captured;
  try {
    const args = buildRunArgs(caseArgs.args, options, "json", mocks.args);
    result = await capture(resolveBin(options.bin), args, options.cwd);
  } finally {
    mocks.cleanup();
    caseArgs.cleanup();
  }
  raiseForStatus(result);
  const report = parse<Report>(result.stdout);
  const caseMocks = isTestCaseInput(caseInput) ? (caseInput.mocks ?? []) : [];
  const bound = [...caseMocks, ...(options.mocks ?? [])];
  if (bound.length > 0) bindMocks(bound, report.runs);
  return report;
}

/** Validate a skill directory (or a folder of them) and return findings. */
export async function validateSkill(
  path: string,
  options: Pick<RunOptions, "bin" | "cwd"> = {},
): Promise<ValidationReport> {
  const args = ["validate", path, "--format", "json"];
  const result = await capture(resolveBin(options.bin), args, options.cwd);
  raiseForStatus(result);
  return parse<ValidationReport>(result.stdout);
}
