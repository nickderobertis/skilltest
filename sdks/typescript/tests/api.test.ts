import { beforeAll, describe, expect, it } from "vitest";
import {
  SkilltestProviderError,
  SkilltestUsageError,
  assistantText,
  runSkill,
  validateSkill,
} from "../src/index.js";
import { caseFile, requireBinaries, skillDir } from "./helpers.js";

beforeAll(() => {
  requireBinaries();
});

describe("runSkill", () => {
  it("passes the happy path and exposes the transcript", async () => {
    const report = await runSkill(caseFile("greet_pass.yaml"));
    expect(report.passed).toBe(true);
    expect(report.summary.runs).toBe(1);
    // Deterministic mix-in check on top of the natural-language evals.
    const run = report.runs[0];
    expect(run).toBeDefined();
    if (!run) return;
    expect(assistantText(run.transcript)).toContain("Dr. Smith");
  });

  it("returns a typed numeric detail above threshold", async () => {
    const report = await runSkill(caseFile("greet_numeric.yaml"));
    expect(report.passed).toBe(true);
    const detail = report.runs[0]?.evals[0]?.detail;
    expect(detail?.kind).toBe("numeric");
    if (detail?.kind === "numeric") {
      expect(detail.value).toBeGreaterThanOrEqual(detail.threshold);
    }
  });

  it("reports a failing case rather than throwing", async () => {
    const report = await runSkill(caseFile("greet_fail.yaml"));
    expect(report.passed).toBe(false);
    expect(report.summary.failed).toBeGreaterThanOrEqual(1);
  });

  it("runs a multi-turn case to its done condition", async () => {
    const report = await runSkill(caseFile("booking_multiturn.yaml"));
    expect(report.passed).toBe(true);
    expect(report.runs[0]?.turns).toBe(2);
  });

  it("throws a provider error when the provider is missing", async () => {
    await expect(
      runSkill(caseFile("greet_pass.yaml"), { provider: "/nonexistent/provider-bin" }),
    ).rejects.toBeInstanceOf(SkilltestProviderError);
  });

  it("throws a provider error when the binary is missing", async () => {
    await expect(
      runSkill(caseFile("greet_pass.yaml"), { bin: "/nonexistent/skilltest-bin" }),
    ).rejects.toBeInstanceOf(SkilltestProviderError);
  });
});

describe("validateSkill", () => {
  it("accepts a good skill", async () => {
    const result = await validateSkill(skillDir("greeter"));
    expect(result.valid).toBe(true);
    expect(result.findings).toHaveLength(0);
  });

  it("rejects an invalid skill with findings", async () => {
    const result = await validateSkill(skillDir("invalid"));
    expect(result.valid).toBe(false);
    expect(result.findings.some((f) => f.message.includes("description"))).toBe(true);
  });
});

describe("usage errors", () => {
  it("throws on a malformed case", async () => {
    // The greeter skill dir has no `evals`, so loading it as a case is invalid.
    await expect(runSkill(skillDir("greeter"))).rejects.toBeInstanceOf(SkilltestUsageError);
  });
});
