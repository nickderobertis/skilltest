/**
 * Ordered stub responses through the plugin's re-exported `stub`: a
 * code-defined two-turn case registered with `skillTest`, whose mock answers
 * the poller's four status checks in order and repeats the last.
 */
import { expect, test } from "vitest";
import { called, skillTest, stub, testCase, user } from "../src/index.js";
// Importing helpers first sets the SKILLTEST_BIN / SKILLTEST_PROVIDER env
// defaults the helper relies on.
import { skillDir } from "./helpers.js";

const status = stub({
  contains: "jobctl status",
  responses: ["queued", { output: "failed", exitCode: 1 }, "passed"],
});

skillTest(
  "poller sees each status in turn (ordered stub responses)",
  testCase({
    skill: skillDir("poller"),
    input: "Watch build-42 until it settles.",
    user: user("You are an operator.\nsay: Check again, please.", { maxTurns: 2 }),
    mocks: [status],
    evals: [called(status, { times: 4 })],
  }),
);

test("the skillTest run bound every intercepted call to the stub", () => {
  expect(status.callCount).toBe(4);
  expect(status.calls.every((call) => call.action === "stub")).toBe(true);
});
