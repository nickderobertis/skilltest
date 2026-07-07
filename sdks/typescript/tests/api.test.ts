import { chmodSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { beforeAll, describe, expect, it } from "vitest";
import {
  SkilltestProviderError,
  SkilltestTimeoutError,
  SkilltestUsageError,
  assistantText,
  runSkill,
  streamSkill,
  toolCalls,
  validateSkill,
} from "../src/index.js";
import { caseFile, requireBinaries, skillDir } from "./helpers.js";

/**
 * Write a config pointing the oneharness provider at a scripted fake emitting
 * `resultsJson`, so a classified failure can be exercised offline. Returns the
 * config path and a cleanup thunk.
 */
function fakeOneharnessConfig(resultsJson: string): { config: string; cleanup: () => void } {
  const dir = mkdtempSync(join(tmpdir(), "skilltest-ts-err-"));
  const oh = join(dir, "oneharness");
  writeFileSync(oh, `#!/bin/sh\ncat >/dev/null\nprintf '%s' '${resultsJson}'\n`);
  chmodSync(oh, 0o755);
  const config = join(dir, "skilltest.yaml");
  writeFileSync(
    config,
    `provider:\n  kind: oneharness\n  bin: ${oh}\n  judge_harness: claude-code\n  timeout_secs: 5\nplatforms: [claude-code]\nmodels: [sonnet]\n`,
  );
  return { config, cleanup: () => rmSync(dir, { recursive: true, force: true }) };
}

/**
 * Like {@link fakeOneharnessConfig}, but the fake speaks the `run --stream`
 * NDJSON protocol and ends in a failed result of the given `status`, so the
 * streaming failure path can be exercised offline.
 */
function fakeOneharnessStreamConfig(status: string): { config: string; cleanup: () => void } {
  const dir = mkdtempSync(join(tmpdir(), "skilltest-ts-stream-err-"));
  const oh = join(dir, "oneharness");
  const results = `{"results":[{"status":"${status}","stderr":"deadline exceeded"}]}`;
  const line = `{"type":"result","report":${results}}`;
  writeFileSync(oh, `#!/bin/sh\ncat >/dev/null\nprintf '%s\\n' '${line}'\n`);
  chmodSync(oh, 0o755);
  const config = join(dir, "skilltest.yaml");
  writeFileSync(
    config,
    `provider:\n  kind: oneharness\n  bin: ${oh}\n  judge_harness: claude-code\n  timeout_secs: 5\nplatforms: [claude-code]\nmodels: [sonnet]\n`,
  );
  return { config, cleanup: () => rmSync(dir, { recursive: true, force: true }) };
}

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

  it("exposes normalized tool calls for analysis", async () => {
    const report = await runSkill(caseFile("tool_events.yaml"));
    const run = report.runs[0];
    expect(run).toBeDefined();
    if (!run) return;
    const calls = toolCalls(run.transcript);
    expect(calls.map((c) => c.name)).toEqual(["edit_file", "bash"]);
    expect(calls[1]?.input).toEqual({ command: 'git commit -m "update config"' });
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

  it("streams tool events then exposes the report", async () => {
    const stream = streamSkill(caseFile("tool_events.yaml"));
    const names: (string | null | undefined)[] = [];
    for await (const ev of stream) {
      expect(ev.case).toBe("tool_events");
      expect(ev.turn).toBe(1);
      names.push(ev.event.name);
    }
    expect(names).toEqual(["edit_file", "bash"]);
    expect(stream.report?.passed).toBe(true);
  });

  it("short-circuits the stream on break", async () => {
    const stream = streamSkill(caseFile("tool_events.yaml"));
    let seen = 0;
    for await (const _ev of stream) {
      seen++;
      break; // abort after the first event
    }
    expect(seen).toBe(1);
  });

  it("throws the kind-specific subclass when a streamed run fails", async () => {
    // A provider failure during a streamed run surfaces the same kind-specific
    // exception as the buffered API: the terminal `{"type":"error",…}` NDJSON
    // line carries the classified kind, thrown once the stream is drained.
    const { config, cleanup } = fakeOneharnessStreamConfig("timeout");
    const savedProvider = process.env.SKILLTEST_PROVIDER;
    // biome-ignore lint/performance/noDelete: the env var must be truly removed, not set to "undefined"
    delete process.env.SKILLTEST_PROVIDER;
    try {
      let caught: unknown;
      try {
        for await (const _ev of streamSkill(caseFile("greet_pass.yaml"), { config })) {
          // drain — the failure surfaces at the end of the stream
        }
      } catch (err) {
        caught = err;
      }
      expect(caught).toBeInstanceOf(SkilltestTimeoutError);
      expect(caught).toBeInstanceOf(SkilltestProviderError);
      const err = caught as SkilltestTimeoutError;
      expect(err.kind).toBe("timeout");
      expect(err.context).toBe("oneharness:claude-code");
    } finally {
      if (savedProvider !== undefined) process.env.SKILLTEST_PROVIDER = savedProvider;
      cleanup();
    }
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

  it("throws the kind-specific subclass on a classified provider failure", async () => {
    // oneharness reports a deadline as `status: "timeout"`; the SDK throws the
    // kind-specific SkilltestTimeoutError (still a SkilltestProviderError) so a
    // handler can `instanceof`-check one category, not parse the message.
    const { config, cleanup } = fakeOneharnessConfig(
      '{"results":[{"status":"timeout","stderr":"deadline exceeded"}]}',
    );
    // The env default would pass `--provider`, overriding the config's oneharness
    // provider — drop it so the fake oneharness bin applies. `delete` (not `=
    // undefined`, which sets the string "undefined") truly unsets it.
    const savedProvider = process.env.SKILLTEST_PROVIDER;
    // biome-ignore lint/performance/noDelete: the env var must be truly removed, not set to "undefined"
    delete process.env.SKILLTEST_PROVIDER;
    try {
      let caught: unknown;
      await runSkill(caseFile("greet_pass.yaml"), { config }).catch((err) => {
        caught = err;
      });
      expect(caught).toBeInstanceOf(SkilltestTimeoutError);
      expect(caught).toBeInstanceOf(SkilltestProviderError);
      const err = caught as SkilltestTimeoutError;
      expect(err.kind).toBe("timeout");
      expect(err.context).toBe("oneharness:claude-code");
    } finally {
      if (savedProvider !== undefined) process.env.SKILLTEST_PROVIDER = savedProvider;
      cleanup();
    }
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
