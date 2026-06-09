/**
 * Run the `skilltest` CLI as a subprocess and parse its JSON contract.
 *
 * This is the code-level API a vitest test reaches for: call {@link runSkill},
 * get a typed {@link Report}, assert on `report.passed`, and mix in deterministic
 * checks against the transcript.
 */
import { spawn } from "node:child_process";
import type { z } from "zod";
import { SkilltestError, SkilltestProviderError, SkilltestUsageError } from "./errors.js";
import {
  type Report,
  ReportSchema,
  type ValidationReport,
  ValidationReportSchema,
} from "./schema.js";

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
}

interface Captured {
  status: number | null;
  stdout: string;
  stderr: string;
}

function resolveBin(bin: string | undefined): string {
  return bin ?? process.env[ENV_BIN] ?? "skilltest";
}

function resolveProvider(provider: string | string[] | undefined): string | undefined {
  const value = provider ?? process.env[ENV_PROVIDER];
  if (value === undefined) return undefined;
  return Array.isArray(value) ? value.join(" ") : value;
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
  const detail = result.stderr.trim() || result.stdout.trim();
  if (result.status === 2) throw new SkilltestUsageError(detail);
  if (result.status === 3) throw new SkilltestProviderError(detail);
  throw new SkilltestError(`skilltest exited ${result.status}: ${detail}`);
}

function parse<T>(schema: z.ZodType<T>, stdout: string): T {
  let json: unknown;
  try {
    json = JSON.parse(stdout);
  } catch (err) {
    throw new SkilltestError(`skilltest did not emit JSON: ${(err as Error).message}`);
  }
  const result = schema.safeParse(json);
  if (!result.success) {
    throw new SkilltestError(
      `skilltest output did not match the expected schema: ${result.error.message}`,
    );
  }
  return result.data;
}

/**
 * Run one or more test cases and return the parsed {@link Report}. A failing
 * eval is reported in `report.passed`, not thrown; only bad input
 * ({@link SkilltestUsageError}) and provider failures
 * ({@link SkilltestProviderError}) throw.
 */
export async function runSkill(casePath: string, options: RunOptions = {}): Promise<Report> {
  const args: string[] = [];
  if (options.config) args.push("--config", options.config);
  args.push("run", casePath, "--format", "json");

  const provider = resolveProvider(options.provider);
  if (provider !== undefined) args.push("--provider", provider);
  for (const platform of options.platforms ?? []) args.push("--platform", platform);
  for (const model of options.models ?? []) args.push("--model", model);
  if (options.judgeModel) args.push("--judge-model", options.judgeModel);
  if (options.maxTurns !== undefined) args.push("--max-turns", String(options.maxTurns));

  const result = await capture(resolveBin(options.bin), args, options.cwd);
  raiseForStatus(result);
  return parse(ReportSchema, result.stdout);
}

/** Validate a skill directory (or a folder of them) and return findings. */
export async function validateSkill(
  path: string,
  options: Pick<RunOptions, "bin" | "cwd"> = {},
): Promise<ValidationReport> {
  const args = ["validate", path, "--format", "json"];
  const result = await capture(resolveBin(options.bin), args, options.cwd);
  raiseForStatus(result);
  return parse(ValidationReportSchema, result.stdout);
}
