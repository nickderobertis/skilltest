/**
 * SDK mock/spy objects through the REAL oneharness binary (the hermetic middle
 * tier, see docs/e2e.md): the fake-claude shim stands in for the harness and
 * executes the ephemerally installed mock hook, so `runSkill({ mocks })` is
 * proven against the real seam — rules compile, `--mocks` delivery, the real
 * `oneharness mock` responder, the spy JSONL, and binding.
 *
 * Skipped (not failed) when `oneharness` is not on PATH; install it with
 * `just install-oneharness` to run these locally.
 */
import { execFileSync } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, test } from "vitest";
import { contains, matching, runSkill, spy, stub } from "../src/index.js";
import { FIXTURES } from "./helpers.js";

function hasOneharness(): boolean {
  try {
    execFileSync("oneharness", ["--version"], { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
}

const OH_FIXTURES = join(FIXTURES, "oneharness");
const CASE = join(OH_FIXTURES, "cases", "spy_plain.yaml");

describe.skipIf(!hasOneharness())("mocks through real oneharness", () => {
  let config: string;
  let savedProvider: string | undefined;

  beforeAll(() => {
    // Drop the fake-provider env default (the config's oneharness provider
    // applies) and route the claude-code harness to the scripted shim.
    savedProvider = process.env.SKILLTEST_PROVIDER;
    // biome-ignore lint/performance/noDelete: assigning undefined to process.env coerces to the string "undefined" — the variable must be truly unset
    delete process.env.SKILLTEST_PROVIDER;
    process.env.ONEHARNESS_BIN_CLAUDE_CODE = join(OH_FIXTURES, "fake-claude.sh");
    const dir = mkdtempSync(join(tmpdir(), "skilltest-oh-ts-"));
    config = join(dir, "skilltest.yaml");
    writeFileSync(
      config,
      [
        "provider:",
        "  kind: oneharness",
        "  bin: oneharness",
        "  judge_harness: claude-code",
        "  timeout_secs: 60",
        "platforms: [claude-code]",
        "models: [fake-model]",
        "judge_model: fake-model",
        "",
      ].join("\n"),
    );
  });

  afterAll(() => {
    if (savedProvider !== undefined) process.env.SKILLTEST_PROVIDER = savedProvider;
    // biome-ignore lint/performance/noDelete: same coercion hazard as above — a real unset is required
    delete process.env.ONEHARNESS_BIN_CLAUDE_CODE;
  });

  test("sdk mocks bind through the real seam", async () => {
    const push = stub({ pattern: /git push( --force)?\b/, output: "Everything up-to-date" });
    const git = spy({ tool: "bash", pattern: /\bgit\b/ });

    const report = await runSkill(CASE, { config, mocks: [push, git] });

    expect(report.passed).toBe(true);
    // The real spy JSONL round-tripped: the stub keeps the ORIGINAL input.
    expect(push.callCount).toBe(1);
    expect(push.calls[0]?.command).toBe("git push origin main");
    expect(push.calls[0]?.mocked).toBe(true);
    expect(git.callCount).toBe(2);
    expect(git.where({ command: matching(/status/) }).called).toBe(true);
    // The canned output reached the (shimmed) model via the real responder's
    // printf rewrite, which the shim executed.
    expect(report.runs[0]?.transcript.messages[1]?.content).toContain("Everything up-to-date");
    // Real oneharness usage flowed through.
    expect(report.runs[0]?.usage?.input_tokens ?? 0).toBeGreaterThan(0);
  });

  test("sdk spy-only observes through the real seam", async () => {
    const shell = spy({ tool: "bash" });
    const report = await runSkill(CASE, { config, mocks: [shell] });
    expect(report.passed).toBe(true);
    expect(shell.callCount).toBe(3);
    expect(shell.calls.every((c) => !c.mocked)).toBe(true);
    expect(shell.where({ command: contains("rm -rf") }).callCount).toBe(1);
  });
});
