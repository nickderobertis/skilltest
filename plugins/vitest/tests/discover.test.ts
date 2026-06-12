import { beforeAll } from "vitest";
import { discover } from "../src/index.js";
// Importing helpers first sets the SKILLTEST_BIN / SKILLTEST_PROVIDER env
// defaults the runner relies on.
import { collectedDir, requireBinaries } from "./helpers.js";

beforeAll(() => {
  requireBinaries();
});

// Auto-discovery: one call registers a vitest test per *.skilltest.yaml found
// under the directory — the recommended setup when vitest is the primary runner.
discover(collectedDir());
