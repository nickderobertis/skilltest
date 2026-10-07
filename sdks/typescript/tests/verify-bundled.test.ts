/**
 * `scripts/verify-bundled.mjs` — publish.yml's release-time npm install proof —
 * passes only on an install whose bundled CLI really answers.
 *
 * A consumer project is laid out the way `npm install @skill-test/sdk` leaves it
 * on this host: the SDK compiled from this tree under
 * `node_modules/@skill-test/sdk`, and the host's `@skill-test/cli-*` package
 * carrying the CLI the project's test-e2e target built. The script then runs
 * as the release job runs it — `node`, from the consumer's directory, with
 * SKILLTEST_BIN unset — through its pass and each way it must refuse.
 */
import { execFileSync, spawnSync } from "node:child_process";
import {
  chmodSync,
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, join } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { platformPackage } from "../src/runner.js";
import { FIXTURES, REPO_ROOT, SKILLTEST_BIN } from "./helpers.js";

const SDK = join(REPO_ROOT, "sdks", "typescript");
const VERIFY = join(SDK, "scripts", "verify-bundled.mjs");
const GREETER = join(FIXTURES, "skills", "greeter");
const INVALID = join(FIXTURES, "skills", "invalid");

const work = mkdtempSync(join(tmpdir(), "skilltest-verify-bundled-"));
const consumer = join(work, "consumer");
let version = "";

beforeAll(() => {
  const cli = process.env.SKILLTEST_BIN ?? SKILLTEST_BIN;
  version = execFileSync(cli, ["--version"], { encoding: "utf8" }).trim().replace("skilltest ", "");

  const sdk = join(consumer, "node_modules", "@skill-test", "sdk");
  mkdirSync(sdk, { recursive: true });
  copyFileSync(join(SDK, "package.json"), join(sdk, "package.json"));
  execFileSync(
    "pnpm",
    ["exec", "tsc", "-p", "tsconfig.build.json", "--outDir", join(sdk, "dist")],
    {
      cwd: SDK,
    },
  );

  const host = join(consumer, "node_modules", ...platformPackage().split("/"));
  mkdirSync(join(host, "bin"), { recursive: true });
  copyFileSync(
    join(SDK, "platforms", platformPackage().replace("@skill-test/", ""), "package.json"),
    join(host, "package.json"),
  );
  copyFileSync(cli, join(host, "bin", "skilltest"));
  chmodSync(join(host, "bin", "skilltest"), 0o755);
  writeFileSync(
    join(consumer, "package.json"),
    JSON.stringify({ name: "consumer", private: true }),
  );
}, 120_000);

afterAll(() => rmSync(work, { recursive: true, force: true }));

function verify(args: string[], options: { cwd?: string; path?: string } = {}) {
  const env = { ...process.env, PATH: options.path ?? process.env.PATH ?? "" };
  // A consumer has neither the gate's SKILLTEST_BIN nor the NODE_PATH `pnpm exec` sets,
  // which would let the workspace's own packages answer for the consumer's.
  Reflect.deleteProperty(env, "SKILLTEST_BIN");
  Reflect.deleteProperty(env, "NODE_PATH");
  return spawnSync("node", [VERIFY, ...args], {
    cwd: options.cwd ?? consumer,
    env,
    encoding: "utf8",
  });
}

describe("verify-bundled.mjs", () => {
  it("passes on an install whose bundled CLI reports the release and validates a skill", () => {
    const ran = verify([version, GREETER]);

    expect(ran.stderr).toBe("");
    expect(ran.status).toBe(0);
    expect(ran.stdout).toContain(`verify-bundled: skilltest ${version} bundled at ${consumer}`);
  });

  it("refuses a bundle of another release", () => {
    const ran = verify(["9.9.9", GREETER]);

    expect(ran.status).toBe(1);
    expect(ran.stderr).toContain("not 'skilltest 9.9.9'; install @skill-test/sdk@9.9.9");
  });

  it("refuses a skill the bundled CLI finds invalid", () => {
    const ran = verify([version, INVALID]);

    expect(ran.status).toBe(1);
    expect(ran.stderr).toContain("missing a non-empty `description`");
  });

  it("refuses when a skilltest on PATH could have answered instead", () => {
    const impostorDir = mkdtempSync(join(work, "impostor-"));
    const impostor = join(impostorDir, "skilltest");
    writeFileSync(impostor, "#!/bin/sh\necho skilltest 0.0.0\n");
    chmodSync(impostor, 0o755);

    const ran = verify([version, GREETER], {
      path: `${impostorDir}${delimiter}${process.env.PATH}`,
    });

    expect(ran.status).toBe(1);
    expect(ran.stderr).toContain(`a skilltest is on PATH (${impostor})`);
  });

  it("refuses a project without the host's platform package", () => {
    const bare = mkdtempSync(join(work, "bare-"));
    writeFileSync(join(bare, "package.json"), readFileSync(join(consumer, "package.json")));

    const ran = verify([version, GREETER], { cwd: bare });

    expect(ran.status).toBe(1);
    expect(ran.stderr).toContain(`${platformPackage()} is not installed in ${bare}`);
  });

  it("refuses malformed arguments", () => {
    for (const args of [[version], [version, join(work, "absent")]]) {
      const ran = verify(args);

      expect(ran.status).toBe(1);
      expect(ran.stderr).toContain("usage: verify-bundled.mjs <expected-version> <skill-dir>");
    }
  });
});
