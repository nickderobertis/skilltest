import { beforeAll, describe, expect, it } from "vitest";
import { compileCase } from "../src/case.js";
import {
  SkilltestUsageError,
  boolean,
  called,
  contains,
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

  it("resolves evals referencing the mock/spy object directly", async () => {
    // No string names to keep in sync: `called`/`notCalled` take the
    // spy/stub object itself and resolve to its compiled declaration name —
    // including an unnamed spy, which gets a declaration only because an
    // eval references it. `where` narrows an object ref just like a string ref.
    const push = stub({ pattern: /git push( --force)?\b/, output: "Everything up-to-date" });
    const sudo = spy({ contains: "sudo" });
    const report = await runSkill({
      skill: skillDir("deployer"),
      input: "Deploy the app",
      mocks: [push, sudo],
      evals: [
        boolean("the reply reports `Everything up-to-date`"),
        called(push, { times: 1, where: { command: contains("origin") } }),
        notCalled(sudo),
      ],
    });
    expect(report.passed).toBe(true);
    expect(push.callCount).toBe(1);
    expect(sudo.called).toBe(false);
  });

  it("compiles object refs to the assigned declaration names", () => {
    // The compile-level pin for object references: unnamed mocks/spies get
    // the positional auto-name, named ones keep their name, and every
    // referencing eval carries the resolved string.
    const push = stub({ pattern: /git push\b/, output: "ok" });
    const sudo = spy({ contains: "sudo" });
    const net = spy({ contains: "curl", name: "net" });
    const compiled = compileCase({
      skill: "skills/deployer",
      input: "Deploy the app",
      mocks: [push, sudo, net],
      evals: [called(push), notCalled(sudo), notCalled(net)],
    });
    expect(compiled.mocks?.map((m) => m.name)).toEqual(["__case_mock_0", "__case_mock_1", "net"]);
    expect(compiled.evals.map((e) => ("mock" in e ? e.mock : null))).toEqual([
      "__case_mock_0",
      "__case_mock_1",
      "net",
    ]);
  });

  it("throws when an eval references a mock outside the case", async () => {
    // An object reference only resolves against the case's own `mocks` — a
    // forgotten entry must be a loud usage error, never a vacuous pass.
    const orphan = spy({ contains: "sudo" });
    await expect(
      runSkill({
        skill: skillDir("deployer"),
        input: "Deploy the app",
        evals: [notCalled(orphan)],
      }),
    ).rejects.toThrowError(/not in this case's `mocks`/);
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

  it("composes case-level and run-level mocks, run-level winning", async () => {
    // Run-level declarations are prepended (first match wins), so the
    // test-local rule shadows the case's own — and both objects bind so the
    // shadowing is assertable.
    const casePush = stub({
      pattern: /git push( --force)?\b/,
      output: "from-case",
      name: "push",
    });
    const runPush = stub({ pattern: /git push( --force)?\b/, output: "from-run-level" });
    const report = await runSkill(
      {
        skill: skillDir("deployer"),
        input: "Deploy the app",
        mocks: [casePush],
        evals: [boolean("the reply reports `from-run-level`")],
      },
      { mocks: [runPush] },
    );
    expect(report.passed).toBe(true);
    expect(runPush.callCount).toBe(1);
    expect(casePush.called).toBe(false);
  });

  it("turns on the observation channel with the bare spy flag", async () => {
    // `spy: true` alone (no mocks) surfaces `mock_calls`, so "channel on with
    // zero interceptions" is distinguishable from "channel off".
    const report = await runSkill({
      skill: skillDir("deployer"),
      input: "Deploy the app",
      spy: true,
      evals: [boolean("the reply says `Deployment finished.`")],
    });
    expect(report.passed).toBe(true);
    const records = report.runs[0]?.mock_calls;
    expect(records).toHaveLength(3);
    expect(records?.every((r) => r.action === "allow")).toBe(true);

    const plain = await runSkill({
      skill: skillDir("deployer"),
      input: "Deploy the app",
      evals: [boolean("the reply says `Deployment finished.`")],
    });
    // The channel-off encoding is "absent or null", never an empty array.
    expect(plain.runs[0]?.mock_calls ?? null).toBeNull();
  });

  it("binds case-level mocks when a stream completes", async () => {
    // A *named* mock passed by object: the eval resolves to the given name,
    // and the streaming path compiles the case identically to the buffered one.
    const push = stub({
      pattern: /git push( --force)?\b/,
      output: "Everything up-to-date",
      name: "push",
    });
    const stream = streamSkill({
      skill: skillDir("deployer"),
      input: "Deploy the app",
      mocks: [push],
      evals: [boolean("the reply reports `Everything up-to-date`"), called(push, { times: 1 })],
    });
    for await (const _ of stream) {
      // drain
    }
    expect(stream.report?.passed).toBe(true);
    expect(push.callCount).toBe(1);
    expect(push.calls[0]?.command).toBe("git push origin main");
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
