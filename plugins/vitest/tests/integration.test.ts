import { skillTest } from "../src/index.js";
// Exercise the one-line vitest helper. Importing helpers first sets the
// SKILLTEST_BIN / SKILLTEST_PROVIDER env defaults the helper relies on.
import { caseFile } from "./helpers.js";

skillTest("greeter confirms the appointment", caseFile("greet_pass.yaml"));
