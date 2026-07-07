/**
 * Unit tests for binary resolution: the precedence chain (explicit > env >
 * bundled platform package > PATH) and that the bundled binary inside the
 * matching optional platform package is discovered when present.
 *
 * In a dev checkout the host's platform package links but ships no binary, so
 * `bundledBin()` is undefined and the runner falls back — exactly how the e2e
 * suite reaches the locally built CLI via `$SKILLTEST_BIN`.
 */
import { chmodSync, existsSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { afterEach, beforeAll, beforeEach, describe, expect, it } from "vitest";
import {
  ENV_BIN,
  ENV_ONEHARNESS_BIN,
  bundledBin,
  bundledOneharness,
  childEnv,
  platformPackage,
  resolveBin,
} from "../src/runner.js";

const require = createRequire(import.meta.url);

// The host platform package links in the workspace but ships no binary in a dev
// checkout; wipe its bin/ so the suite is deterministic even if a local publish
// dry-run left one staged there (the dir is git-ignored).
const hostPkgBin = join(dirname(require.resolve(`${platformPackage()}/package.json`)), "bin");
beforeAll(() => rmSync(hostPkgBin, { recursive: true, force: true }));

describe("resolveBin precedence", () => {
  const saved = process.env[ENV_BIN];
  afterEach(() => {
    if (saved === undefined) delete process.env[ENV_BIN];
    else process.env[ENV_BIN] = saved;
  });

  it("prefers an explicit bin over everything", () => {
    process.env[ENV_BIN] = "/from/env";
    expect(resolveBin("/explicit")).toBe("/explicit");
  });

  it("uses $SKILLTEST_BIN over the bundled binary and PATH", () => {
    process.env[ENV_BIN] = "/from/env";
    expect(resolveBin(undefined)).toBe("/from/env");
  });

  it("falls back to `skilltest` on PATH when nothing is set or bundled", () => {
    delete process.env[ENV_BIN];
    // No binary is bundled in a dev checkout.
    expect(bundledBin()).toBeUndefined();
    expect(resolveBin(undefined)).toBe("skilltest");
  });
});

describe("bundledBin", () => {
  // Drop a binary into the host package to prove the lookup finds it, then clean
  // up (the dir is git-ignored).
  const binDir = hostPkgBin;
  const exe = process.platform === "win32" ? "skilltest.exe" : "skilltest";
  const binPath = join(binDir, exe);

  beforeEach(() => {
    rmSync(binDir, { recursive: true, force: true });
  });
  afterEach(() => {
    rmSync(binDir, { recursive: true, force: true });
  });

  it("returns undefined when the package ships no binary", () => {
    expect(bundledBin()).toBeUndefined();
  });

  it("finds the binary bundled in the matching platform package", () => {
    mkdirSync(binDir, { recursive: true });
    writeFileSync(binPath, "#!/bin/sh\n");
    chmodSync(binPath, 0o755);
    expect(bundledBin()).toBe(binPath);
    delete process.env[ENV_BIN];
    expect(resolveBin(undefined)).toBe(binPath);
  });
});

describe("oneharness resolution", () => {
  const saved = process.env[ENV_ONEHARNESS_BIN];
  afterEach(() => {
    if (saved === undefined) delete process.env[ENV_ONEHARNESS_BIN];
    else process.env[ENV_ONEHARNESS_BIN] = saved;
  });

  it("resolves the launcher shipped by the oneharness-cli dependency", () => {
    // Unlike the skilltest platform package, oneharness-cli ships a real launcher
    // in a dev checkout, so this resolves to a present file.
    const launcher = bundledOneharness();
    expect(launcher).toBeDefined();
    expect(launcher?.endsWith(join("bin", "oneharness.js"))).toBe(true);
    expect(existsSync(launcher as string)).toBe(true);
  });

  it("points SKILLTEST_ONEHARNESS_BIN at the bundled launcher when unset", () => {
    delete process.env[ENV_ONEHARNESS_BIN];
    expect(childEnv()[ENV_ONEHARNESS_BIN]).toBe(bundledOneharness());
  });

  it("leaves a caller-set SKILLTEST_ONEHARNESS_BIN untouched", () => {
    process.env[ENV_ONEHARNESS_BIN] = "/my/oneharness";
    expect(childEnv()[ENV_ONEHARNESS_BIN]).toBe("/my/oneharness");
  });
});
