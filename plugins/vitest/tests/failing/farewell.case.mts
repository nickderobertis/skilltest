// Run by the child vitest in failure.test.ts: the greeter never says goodbye,
// so this registered test must fail with the judge's reason.
import { skillTest } from "../../src/index.js";
import { caseFile, requireBinaries } from "../helpers.js";

requireBinaries();
skillTest("greeter says goodbye", caseFile("greet_fail.yaml"));
