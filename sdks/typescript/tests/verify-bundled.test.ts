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
  cpSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
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

// Resolved, because the verifier reports paths under its working directory
// and macOS's tmpdir (/var/folders/...) is a symlink into /private/var.
const work = realpathSync(mkdtempSync(join(tmpdir(), "skilltest-verify-bundled-")));
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

/** A copy of the consumer project to damage for one refusal. */
function copyOfConsumer(tag: string): { dir: string; hostBin: string } {
  const dir = mkdtempSync(join(work, `${tag}-`));
  cpSync(consumer, dir, { recursive: true });
  return {
    dir,
    hostBin: join(dir, "node_modules", ...platformPackage().split("/"), "bin", "skilltest"),
  };
}

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

// llmlint: ignore[shell_test_tiers_stay_split] this tier is the one that drives the built CLI by design (every suite here runs target/debug/skilltest via SKILLTEST_BIN); the script under test is this SDK's own install proof, and it needs only node and the SDK's own tsc build
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

  it("refuses a platform package that carries no binary", () => {
    const { dir, hostBin } = copyOfConsumer("nobin");
    rmSync(hostBin);

    const ran = verify([version, GREETER], { cwd: dir });

    expect(ran.status).toBe(1);
    expect(ran.stderr).toContain(`${platformPackage()} carries no ${hostBin}`);
  });

  it("refuses a bundled binary that does not run", () => {
    const { dir, hostBin } = copyOfConsumer("broken");
    writeFileSync(hostBin, "#!/bin/sh\necho 'cannot execute binary file' >&2\nexit 126\n");

    const ran = verify([version, GREETER], { cwd: dir });

    expect(ran.status).toBe(1);
    expect(ran.stderr).toContain("--version exited 126 (cannot execute binary file)");
  });

  it("refuses a bundled binary that cannot start", () => {
    const { dir, hostBin } = copyOfConsumer("unstartable");
    chmodSync(hostBin, 0o644);

    const ran = verify([version, GREETER], { cwd: dir });

    expect(ran.status).toBe(1);
    expect(ran.stderr).toContain(`${hostBin} --version exited Error: spawnSync ${hostBin} EACCES`);
    expect(ran.stderr).toContain("rebuild");
  });

  it("refuses when validation through the bundle throws", () => {
    const { dir, hostBin } = copyOfConsumer("crash");
    writeFileSync(
      hostBin,
      `#!/bin/sh\n[ "$1" = --version ] && { echo "skilltest ${version}"; exit 0; }\necho 'internal error' >&2\nexit 7\n`,
    );

    const ran = verify([version, GREETER], { cwd: dir });

    expect(ran.status).toBe(1);
    expect(ran.stderr).toContain("through the bundled CLI threw");
    expect(ran.stderr).toContain("rerun it by hand");
  });

  it("refuses an install whose SDK does not load", () => {
    const { dir } = copyOfConsumer("nosdk");
    rmSync(join(dir, "node_modules", "@skill-test", "sdk", "dist"), { recursive: true });

    const ran = verify([version, GREETER], { cwd: dir });

    expect(ran.status).toBe(1);
    expect(ran.stderr).toContain(`@skill-test/sdk does not load from ${dir}`);
  });

  it("refuses malformed arguments", () => {
    for (const args of [[version], [version, join(work, "absent")]]) {
      const ran = verify(args);

      expect(ran.status).toBe(1);
      expect(ran.stderr).toContain("usage: verify-bundled.mjs <expected-version> <skill-dir>");
    }
  });
});
