/**
 * The mock/spy surface through the vitest plugin: `skillTest` passes `mocks`
 * through to the SDK, and after that registered test runs, the mock objects
 * are bound — a following test in the same file (vitest runs a file's tests in
 * order) asserts on them. The declarative side (a case's own `mocks:` block +
 * `called`/`not_called` evals) is covered by `collected/deploy.skilltest.yaml`
 * via the discovery suite.
 */
import { expect, test } from "vitest";
import { contains, matching, runSkill, skillTest, spy, stub } from "../src/index.js";
import { caseFile } from "./helpers.js";

const push = stub({ pattern: /git push( --force)?\b/, output: "Everything up-to-date" });
const git = spy({ tool: "bash", pattern: /\bgit\b/ });

skillTest("deploy with code-level mocks", caseFile("deploy_plain.yaml"), {
  mocks: [push, git],
});

test("the skillTest run bound the mocks", () => {
  expect(push.callCount).toBe(1);
  expect(push.calls[0]?.command).toBe("git push origin main");
  expect(git.callCount).toBe(2);
  expect(git.where({ command: matching(/\bsudo\b/) }).called).toBe(false);
});

test("the re-exported SDK mock API works standalone", async () => {
  const danger = spy({ contains: "rm -rf" });
  await runSkill(caseFile("deploy_plain.yaml"), { mocks: [danger] });
  expect(danger.callCount).toBe(1);
  expect(danger.where({ command: contains("/tmp/build") }).called).toBe(true);
});
