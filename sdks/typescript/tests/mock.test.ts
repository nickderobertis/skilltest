/**
 * The mock/spy API, driven through the real binary + fake provider: construct
 * mocks/spies as variables, pass them to `runSkill`, assert on their typed
 * properties directly.
 *
 * The deployer fixture skill scripts three tool calls (`git push origin main`,
 * `git status`, `rm -rf /tmp/build`), so interception and observation are
 * deterministic.
 */
import { beforeAll, describe, expect, test } from "vitest";
import {
  SkilltestUsageError,
  anything,
  contains,
  deny,
  matching,
  rewrite,
  runSkill,
  spy,
  stub,
} from "../src/index.js";
import { compileDecls } from "../src/mock.js";
import { caseFile, requireBinaries } from "./helpers.js";

beforeAll(requireBinaries);

const DEPLOY = caseFile("deploy_plain.yaml");

describe("mocks and spies", () => {
  test("stub intercepts and binds the original calls", async () => {
    const push = stub({ pattern: /git push( --force)?\b/, output: "Everything up-to-date" });
    const git = spy({ tool: "bash", pattern: /\bgit\b/ });

    const report = await runSkill(DEPLOY, { mocks: [push, git] });

    expect(report.passed).toBe(true);
    expect(push.callCount).toBe(1);
    // The mock's call carries the ORIGINAL input, not the substituted stub.
    expect(push.calls[0]?.command).toBe("git push origin main");
    expect(push.calls[0]?.mocked).toBe(true);
    expect(push.calls[0]?.input).toMatchObject({ command: expect.stringContaining("origin") });
    // The spy observes everything matching — including the mocked push.
    expect(git.callCount).toBe(2);
    // And the canned output surfaced to the model.
    expect(report.runs[0]?.transcript.messages[1]?.content).toContain("Everything up-to-date");
  });

  test("deny blocks and where() filters with the full surface", async () => {
    const danger = deny({ contains: "rm -rf", message: "blocked in tests" });
    const shell = spy({ tool: "bash" });

    await runSkill(DEPLOY, { mocks: [danger, shell] });

    expect(danger.called).toBe(true);
    expect(danger.calls[0]?.action).toBe("deny");
    expect(shell.where({ command: matching(/\bgit\b/) }).callCount).toBe(2);
    // An empty view is a full citizen, and its miss context falls back to the
    // parent's observed calls.
    const quiet = shell.where({ command: contains("sudo") });
    expect(quiet.called).toBe(false);
    expect(quiet.observed).toContain("git push origin main");
    expect(shell.where({ tool: "bash", command: anything() }).callCount).toBe(3);
    // Arbitrary predicates work locally on spies.
    expect(shell.where({ command: (v) => v.startsWith("git") }).callCount).toBe(2);
  });

  test("rewrite substitutes input while the mock keeps the original", async () => {
    const redirect = rewrite({ contains: "git status", input: { command: "true" } });
    const report = await runSkill(DEPLOY, { mocks: [redirect] });
    expect(redirect.callCount).toBe(1);
    expect(redirect.calls[0]?.command).toBe("git status");
    const events = report.runs[0]?.transcript.messages[1]?.events ?? [];
    expect(events.some((e) => JSON.stringify(e.input) === '{"command":"true"}')).toBe(true);
  });

  test("unbound access throws, never reads as zero", () => {
    const lonely = spy({ tool: "bash" });
    expect(() => lonely.calls).toThrow(SkilltestUsageError);
    expect(() => lonely.called).toThrow(/not bound/);
    expect(() => lonely.where({ command: contains("x") })).toThrow(/not bound/);
  });

  test("each run re-binds fresh", async () => {
    const git = spy({ tool: "bash", pattern: /\bgit\b/ });
    await runSkill(DEPLOY, { mocks: [git] });
    const first = git.callCount;
    await runSkill(DEPLOY, { mocks: [git] });
    expect(git.callCount).toBe(first);
  });

  test("construction validates criteria loudly", () => {
    expect(() => spy({})).toThrow(/at least one criterion/);
    expect(() => spy({ contains: "" })).toThrow(/match everything/);
    expect(() => contains("")).toThrow(/non-empty/);
    expect(() => matching("")).toThrow(/non-empty/);
  });

  test("mock criteria must compile to the hook side", () => {
    // A predicate cannot run inside the harness — loud at compile, never a
    // silently narrower mock; anything() is spy-only for the same reason.
    const predicate = stub({
      tool: "bash",
      where: { command: (v) => v.includes("x") },
      output: "y",
    });
    expect(() => compileDecls([predicate])).toThrow(/cannot be compiled/);
    const exists = deny({ tool: "bash", where: { command: anything() }, message: "no" });
    expect(() => compileDecls([exists])).toThrow(/cannot be compiled/);
    // The shipped matchers and exact strings compile fine.
    const ok = stub({
      tool: "bash",
      where: { command: contains("git"), flag: matching(/^-/), mode: "fast" },
      output: "y",
      exitCode: 2,
    });
    const [decl] = compileDecls([ok]);
    expect(decl).toMatchObject({
      name: "__mock_0",
      match: {
        tool: "bash",
        input: { command: { contains: "git" }, flag: { pattern: "^-" }, mode: "fast" },
      },
      stub: { output: "y", exit_code: 2 },
    });
  });
});
