/**
 * Unit tests for binary resolution: the precedence chain (explicit > env >
 * bundled platform package > PATH) and that the bundled binary inside the
 * matching optional platform package is discovered when present.
 *
 * In a dev checkout the host's platform package links but ships no binary, so
 * `bundledBin()` is undefined and the runner falls back — exactly how the e2e
 * suite reaches the locally built CLI via `$SKILLTEST_BIN`.
 */
import { chmodSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { afterEach, beforeAll, beforeEach, describe, expect, it } from "vitest";
import {
  ENV_BIN,
  ENV_ONEHARNESS_BIN,
  ONEHARNESS_PACKAGES,
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

  it("resolves the native oneharness binary, not the node launcher", () => {
    // oneharness-cli's platform package ships the real binary in a dev checkout,
    // so this resolves it directly — exec'd without a Node shim in the hot path.
    const bin = bundledOneharness();
    expect(bin).toBeDefined();
    expect(existsSync(bin as string)).toBe(true);
    const exe = process.platform === "win32" ? "oneharness.exe" : "oneharness";
    expect(bin?.endsWith(join("bin", exe))).toBe(true);
    // The native binary, not the `bin/oneharness.js` launcher.
    expect(bin?.endsWith(".js")).toBe(false);
    expect(bin).toContain("@oneharness");
  });

  it("points SKILLTEST_ONEHARNESS_BIN at the bundled launcher when unset", () => {
    delete process.env[ENV_ONEHARNESS_BIN];
    expect(childEnv()[ENV_ONEHARNESS_BIN]).toBe(bundledOneharness());
  });

  // Resolving and running another host's native oneharness needs that host, which
  // the release's verify-windows job is; what a missing map entry breaks — and what
  // this holds on every host — is the name lookup the resolution above starts from.
  it("maps every platform the SDK ships to a package oneharness-cli publishes", () => {
    const optionalDependencies = (path: string): string[] => {
      const parsed: unknown = JSON.parse(readFileSync(path, "utf8"));
      const deps =
        typeof parsed === "object" && parsed !== null
          ? (parsed as { optionalDependencies?: unknown }).optionalDependencies
          : undefined;
      if (typeof deps !== "object" || deps === null || Array.isArray(deps)) {
        throw new Error(
          `${path} has no optionalDependencies object; restore it from git or reinstall`,
        );
      }
      return Object.keys(deps);
    };
    const shipped = optionalDependencies(require.resolve("../package.json")).map((name) =>
      name.replace("@skill-test/cli-", ""),
    );
    const published = optionalDependencies(require.resolve("oneharness-cli/package.json"));

    expect(shipped.length).toBeGreaterThan(0);
    for (const platform of shipped) {
      expect(published, `oneharness-cli for ${platform}`).toContain(ONEHARNESS_PACKAGES[platform]);
    }
  });

  it("leaves a caller-set SKILLTEST_ONEHARNESS_BIN untouched", () => {
    process.env[ENV_ONEHARNESS_BIN] = "/my/oneharness";
    expect(childEnv()[ENV_ONEHARNESS_BIN]).toBe("/my/oneharness");
  });
});
