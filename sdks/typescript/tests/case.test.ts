import { beforeAll, describe, expect, it } from "vitest";
import {
  SkilltestUsageError,
  boolean,
  called,
  notCalled,
  numeric,
  runSkill,
  spy,
  streamSkill,
  stub,
  testCase,
  user,
} from "../src/index.js";
import { requireBinaries, skillDir } from "./helpers.js";

beforeAll(() => {
  requireBinaries();
});

describe("code-defined cases", () => {
  it("runs an inline case like a YAML case", async () => {
    const report = await runSkill(
      testCase({
        name: "inline_greet",
        skill: skillDir("greeter"),
        input: "Greet Dr. Smith, who has an appointment today.",
        evals: [
          boolean("the reply greets `Dr. Smith` by name"),
          boolean("the reply states the appointment is `confirmed`"),
        ],
      }),
    );
    expect(report.passed).toBe(true);
    expect(report.summary.runs).toBe(1);
    expect(report.runs[0]?.case).toBe("inline_greet");
  });

  it("accepts a plain object (no testCase wrapper) and a numeric eval", async () => {
    const report = await runSkill({
      skill: skillDir("greeter"),
      input: "Greet Dr. Smith",
      evals: [numeric("mentions `Dr. Smith`", { min: 0, max: 10, threshold: 5 })],
    });
    expect(report.passed).toBe(true);
    // An unnamed case defaults to `case`.
    expect(report.runs[0]?.case).toBe("case");
  });

  it("reports a failing eval rather than throwing", async () => {
    const report = await runSkill({
      skill: skillDir("greeter"),
      input: "Greet Dr. Smith",
      evals: [boolean("the reply mentions `a nonexistent phrase`")],
    });
    expect(report.passed).toBe(false);
  });

  it("runs a multi-turn case to its done condition", async () => {
    const report = await runSkill({
      skill: skillDir("greeter"),
      input: "I'd like to confirm my appointment, please.",
      user: user("You are a terse patient confirming an appointment.\nsay: Yes, please go ahead.", {
        doneWhen: "the conversation has reached turns>=2",
        maxTurns: 4,
      }),
      evals: [boolean("the assistant confirmed the appointment (`confirmed`)")],
    });
    expect(report.passed).toBe(true);
    expect(report.runs[0]?.turns).toBe(2);
  });

  it("carries named mocks that back call/notCalled evals and bind for assertions", async () => {
    const push = stub({
      pattern: /git push( --force)?\b/,
      output: "Everything up-to-date",
      name: "push",
    });
    const sudo = spy({ contains: "sudo", name: "sudo" });
    const report = await runSkill({
      skill: skillDir("deployer"),
      input: "Deploy the app",
      mocks: [push, sudo],
      evals: [
        boolean("the reply reports `Everything up-to-date`"),
        called("push", { times: 1 }),
        notCalled("sudo"),
      ],
    });
    expect(report.passed).toBe(true);
    // The case's own mocks bind just like run-level `mocks`.
    expect(push.callCount).toBe(1);
    expect(push.calls[0]?.command).toBe("git push origin main");
    expect(sudo.called).toBe(false);
  });

  it("composes with run-level mocks on an inline case", async () => {
    const git = spy({ tool: "bash", pattern: /\bgit\b/ });
    const report = await runSkill(
      {
        skill: skillDir("deployer"),
        input: "Deploy the app",
        evals: [boolean("the reply says `Deployment finished.`")],
      },
      { mocks: [git] },
    );
    expect(report.passed).toBe(true);
    expect(git.callCount).toBe(2);
  });

  it("streams an inline case", async () => {
    const stream = streamSkill({
      skill: skillDir("tooluser"),
      input: "do the thing",
      evals: [boolean("ok")],
    });
    const names: (string | null | undefined)[] = [];
    for await (const ev of stream) names.push(ev.event.name);
    expect(names.length).toBeGreaterThan(0);
    expect(stream.report?.passed).toBe(true);
  });

  it("throws a usage error on a malformed inline case", async () => {
    await expect(
      runSkill({ skill: skillDir("greeter"), input: "hi", evals: [] }),
    ).rejects.toBeInstanceOf(SkilltestUsageError);
  });
});
