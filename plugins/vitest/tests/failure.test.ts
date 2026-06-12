/**
 * The collector failure path: a registered case whose eval fails must fail the
 * vitest run with the judge's reason. A failing test can't be asserted from
 * inside its own run, so this drives a *child* vitest (see
 * `failing/vitest.config.ts`) end-to-end and asserts on its exit code/output.
 */
import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { beforeAll, expect, it } from "vitest";
import { PLUGIN_ROOT, requireBinaries } from "./helpers.js";

beforeAll(() => {
  requireBinaries();
});

it("a failing collected case fails the run with the judge's reason", () => {
  const vitestBin = join(PLUGIN_ROOT, "node_modules", ".bin", "vitest");
  const result = spawnSync(
    vitestBin,
    ["run", "--config", join("tests", "failing", "vitest.config.ts")],
    { cwd: PLUGIN_ROOT, encoding: "utf8", env: process.env },
  );
  const output = `${result.stdout}\n${result.stderr}`;
  expect(result.status, output).not.toBe(0);
  expect(output).toContain("greeter says goodbye");
  expect(output).toContain("says-goodbye");
}, 60_000);
