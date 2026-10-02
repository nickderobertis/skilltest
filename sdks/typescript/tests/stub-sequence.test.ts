/**
 * Ordered stub responses (`stub({ responses })`), driven through the real
 * binary + fake provider. The poller fixture skill checks `jobctl status
 * build-42` twice per turn, so a two-turn case makes four intercepted calls.
 */
import { beforeAll, describe, expect, it } from "vitest";
import {
  SkilltestUsageError,
  type StubResponseInput,
  called,
  runSkill,
  stub,
  testCase,
  user,
} from "../src/index.js";
import { compileDecls } from "../src/mock.js";
import { requireBinaries, skillDir } from "./helpers.js";

beforeAll(requireBinaries);

const RESPONSES: StubResponseInput[] = [
  "build-42: queued",
  { output: "build-42: failed (retrying)", exitCode: 1 },
  "build-42: passed",
];

describe("ordered stub responses", () => {
  it("answers successive calls in order and repeats the last", async () => {
    const status = stub({ tool: "bash", contains: "jobctl status", responses: RESPONSES });
    const report = await runSkill(
      testCase({
        skill: skillDir("poller"),
        input: "Watch build-42 until it settles.",
        user: user("You are an operator waiting on a build.\nsay: Check again, please.", {
          maxTurns: 2,
        }),
        mocks: [status],
        evals: [called(status, { times: 4 })],
      }),
    );

    expect(report.passed).toBe(true);
    // One recorded stub call per interception, with the original input.
    expect(status.callCount).toBe(4);
    for (const call of status.calls) {
      expect(call.action).toBe("stub");
      expect(call.command).toBe("jobctl status build-42");
    }
    // The count spans both turns and the last response repeats.
    const outputs = (report.runs[0]?.transcript.messages ?? [])
      .filter((m) => m.role === "assistant")
      .flatMap((m) => m.events ?? [])
      .map((e) => (e.output ?? "").trim());
    expect(outputs).toEqual([
      "build-42: queued",
      "build-42: failed (retrying)",
      "build-42: passed",
      "build-42: passed",
    ]);
  });

  it("compiles to the YAML sequence form", () => {
    const [decl] = compileDecls([stub({ contains: "jobctl status", responses: RESPONSES })]);
    expect(decl?.stub).toEqual([
      "build-42: queued",
      { output: "build-42: failed (retrying)", exit_code: 1 },
      "build-42: passed",
    ]);
    const [single] = compileDecls([stub({ contains: "git push", output: "ok" })]);
    expect(single?.stub).toEqual({ output: "ok", exit_code: 0 });
  });

  it.each([
    [{ responses: ["a"], output: "b" }, "not both"],
    [{ responses: ["a"], exitCode: 1 }, "not both"],
    [{}, "needs `output`"],
    [{ responses: [] }, "at least one response"],
    [{ responses: "ab" }, "needs an array"],
    [{ responses: ["a", ""] }, "response 1 must not be empty"],
    [{ responses: [{ output: "a", exit_code: 2 }] }, "unknown key"],
    [{ responses: [{ exitCode: 2 }] }, "string `output`"],
    [{ responses: [{ output: "a", exitCode: 1.5 }] }, "integer `exitCode`"],
    [{ responses: [3] }, "output string"],
  ])("refuses %j at construction", (options, message) => {
    // Typed callers cannot even write most of these; JavaScript callers can.
    const build = stub as (options: object) => unknown;
    expect(() => build({ contains: "jobctl status", ...options })).toThrow(SkilltestUsageError);
    expect(() => build({ contains: "jobctl status", ...options })).toThrow(message);
  });
});
