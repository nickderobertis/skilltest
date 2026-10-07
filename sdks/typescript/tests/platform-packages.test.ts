/**
 * The Windows platform packages pack with the CLI where the SDK resolves it.
 *
 * publish.yml's npm job stages each target's binary into its
 * `@skill-test/cli-<os>-<arch>` package with `scripts/stage-npm-binary.sh`, then
 * publishes it with `npm`. This drives that same staging and an `npm pack` of
 * the two Windows packages from stand-in binaries and reads the tarballs: each
 * must carry `bin/skilltest.exe` — the path `bundledBin()` resolves on win32 —
 * with the staged bytes and its executable bit, under the package's own name
 * and `os`/`cpu` scope. A target the stager does not know is refused.
 */
import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, describe, expect, it } from "vitest";
import { REPO_ROOT } from "./helpers.js";

const STAGER = join(REPO_ROOT, "scripts", "stage-npm-binary.sh");

const WINDOWS = [
  {
    target: "x86_64-pc-windows-msvc",
    dir: "cli-win32-x64",
    name: "@skill-test/cli-win32-x64",
    cpu: "x64",
  },
  {
    target: "aarch64-pc-windows-msvc",
    dir: "cli-win32-arm64",
    name: "@skill-test/cli-win32-arm64",
    cpu: "arm64",
  },
];

interface PackedFile {
  path: string;
  mode: number;
}

interface Packed {
  name: string;
  filename: string;
  files: PackedFile[];
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** `npm pack --json`'s output for one package, checked before any field is read. */
function parsePacked(stdout: string): Packed {
  const parsed: unknown = JSON.parse(stdout);
  const entry: unknown = Array.isArray(parsed) && parsed.length === 1 ? parsed[0] : undefined;
  if (
    !isRecord(entry) ||
    typeof entry.name !== "string" ||
    typeof entry.filename !== "string" ||
    !Array.isArray(entry.files)
  ) {
    throw new Error(`npm pack --json did not describe exactly one package: ${stdout}`);
  }
  const files = entry.files.map((f: unknown): PackedFile => {
    if (!isRecord(f) || typeof f.path !== "string" || typeof f.mode !== "number") {
      throw new Error(`npm pack --json listed a malformed file: ${JSON.stringify(f)}`);
    }
    return { path: f.path, mode: f.mode };
  });
  return { name: entry.name, filename: entry.filename, files };
}

/** The packed package.json's `os`/`cpu` scope, checked to be string arrays. */
function parseScope(text: string): { os: string[]; cpu: string[] } {
  const parsed: unknown = JSON.parse(text);
  const strings = (v: unknown): v is string[] =>
    Array.isArray(v) && v.every((x) => typeof x === "string");
  if (!isRecord(parsed) || !strings(parsed.os) || !strings(parsed.cpu)) {
    throw new Error(`the packed package.json has no string-array os/cpu: ${text}`);
  }
  return { os: parsed.os, cpu: parsed.cpu };
}

const work = mkdtempSync(join(tmpdir(), "skilltest-platform-pack-"));
// The staged bin/ dirs are git-ignored and exist only for a pack; remove the
// ones this suite created so the checkout is left as it was found.
const created: string[] = [];
afterAll(() => {
  for (const dir of created) rmSync(dir, { recursive: true, force: true });
  rmSync(work, { recursive: true, force: true });
});

// llmlint: ignore[shell_test_tiers_stay_split] no nx project owns scripts/stage-npm-binary.sh; it stages this SDK's own platform packages, so its pack test runs in this project's tier, as test_platform_wheels.py does for the wheel scripts
describe("Windows platform packages", () => {
  for (const pkg of WINDOWS) {
    it(`${pkg.name} packs the staged CLI at bin/skilltest.exe`, () => {
      const binDir = join(REPO_ROOT, "sdks", "typescript", "platforms", pkg.dir, "bin");
      if (!existsSync(binDir)) created.push(binDir);
      const standIn = join(work, `${pkg.target}.exe`);
      writeFileSync(standIn, `stand-in for ${pkg.target}`);

      const staged = execFileSync("bash", [STAGER, pkg.target, standIn], { encoding: "utf8" });
      const pkgDir = join(REPO_ROOT, staged.trim());
      expect(pkgDir).toBe(join(REPO_ROOT, "sdks", "typescript", "platforms", pkg.dir));

      const dest = mkdtempSync(join(work, `${pkg.dir}-`));
      const packed = parsePacked(
        execFileSync("npm", ["pack", "--json", "--pack-destination", dest], {
          cwd: pkgDir,
          encoding: "utf8",
        }),
      );

      expect(packed.name).toBe(pkg.name);
      const exe = packed.files.find((f) => f.path === "bin/skilltest.exe");
      expect(exe, JSON.stringify(packed.files)).toBeDefined();
      expect((exe?.mode ?? 0) & 0o111).not.toBe(0);
      expect(packed.files.map((f) => f.path).sort()).toEqual(["bin/skilltest.exe", "package.json"]);

      const tarball = join(dest, packed.filename);
      const read = (member: string) =>
        execFileSync("tar", ["-xOzf", tarball, `package/${member}`], { encoding: "utf8" });
      expect(read("bin/skilltest.exe")).toBe(readFileSync(standIn, "utf8"));
      const manifest = parseScope(read("package.json"));
      expect(manifest.os).toEqual(["win32"]);
      expect(manifest.cpu).toEqual([pkg.cpu]);
    });
  }

  it("refuses a target with no platform package", () => {
    const standIn = join(work, "freebsd");
    writeFileSync(standIn, "stand-in");

    const ran = spawnSync("bash", [STAGER, "x86_64-unknown-freebsd", standIn], {
      encoding: "utf8",
    });

    expect(ran.status).toBe(2);
    expect(ran.stderr).toContain("unsupported target: x86_64-unknown-freebsd");
  });
});
