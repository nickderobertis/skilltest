import { boolean, called, skillTest, stub, testCase } from "../src/index.js";
// Importing helpers first sets the SKILLTEST_BIN / SKILLTEST_PROVIDER env
// defaults the helper relies on.
import { skillDir } from "./helpers.js";

// The recommended form: define the whole case in code and register it as a
// vitest test in one line. The case API is re-exported from the plugin, so a
// vitest suite needs only this one dependency.
skillTest(
  "greeter names the patient (code-defined)",
  testCase({
    skill: skillDir("greeter"),
    input: "Greet Dr. Smith, who has an appointment today.",
    evals: [boolean("the reply greets `Dr. Smith` by name")],
  }),
);

skillTest(
  "deployer pushes exactly once (code-defined with a mock)",
  testCase({
    skill: skillDir("deployer"),
    input: "Deploy the app",
    mocks: [
      stub({ pattern: /git push( --force)?\b/, output: "Everything up-to-date", name: "push" }),
    ],
    evals: [boolean("the reply reports `Everything up-to-date`"), called("push", { times: 1 })],
  }),
);
