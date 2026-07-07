import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { compileCase } from "../src/case.js";
import {
  boolean,
  called,
  contains,
  deny,
  notCalled,
  numeric,
  rewrite,
  spy,
  stub,
  user,
} from "../src/index.js";
import { FIXTURES } from "./helpers.js";

describe("input contract", () => {
  it("builders compile to the kitchen-sink golden", () => {
    // The input-contract pin: a maximal case built with every builder feature
    // must compile to exactly `tests/fixtures/contract/case_kitchen_sink.json`.
    // The same golden is pinned Rust-side (construction + strict parse + an
    // executable e2e run) and by the Python builders, so drift in any
    // direction breaks a named test. If the contract gains a field: update the
    // golden, the Rust construction, and every SDK's builders together.
    const push = stub({
      tool: "bash",
      pattern: /git push( --force)?\b/,
      where: { command: contains("origin") },
      output: "Everything up-to-date",
      name: "push",
    });
    const danger = deny({
      contains: "rm -rf",
      message: "destructive commands are blocked",
      name: "danger",
    });
    const status = rewrite({
      where: { command: "git status" },
      input: { command: "git status --short" },
      name: "status",
    });
    const sudo = spy({ tool: "bash", contains: "sudo", name: "sudo" });

    const compiled = compileCase({
      name: "kitchen_sink",
      skill: "tests/fixtures/skills/deployer",
      input: "Deploy the app",
      user: user("a terse operator\nsay: Yes, proceed.", {
        doneWhen: "the conversation has reached turns>=1",
        maxTurns: 3,
      }),
      mocks: [push, danger, status, sudo],
      evals: [
        boolean("the reply mentions `flying pigs`", { expected: false, name: "no-nonsense" }),
        numeric("mentions `Deployment finished.`", {
          min: 0,
          max: 10,
          threshold: 5,
          comparator: ">",
          name: "finished",
        }),
        // One eval references its mock by *object*, one by string name —
        // the two forms must compile to the identical golden JSON.
        called(push, {
          times: 1,
          where: { command: contains("origin") },
          name: "pushed-once",
        }),
        notCalled("sudo", { name: "no-sudo" }),
      ],
    });

    const golden = JSON.parse(
      readFileSync(join(FIXTURES, "contract", "case_kitchen_sink.json"), "utf8"),
    );
    expect(compiled).toEqual(golden);
  });
});
