// Prove an installed `@skill-test/sdk` runs the CLI bundled in its platform package.
//
//   cd <consumer-project> && node <repo>/scripts/verify-bundled-npm.mjs <expected-version> <skill-dir>
//
// Run it from a fresh project `@skill-test/sdk` was installed into. With
// $SKILLTEST_BIN unset and no `skilltest` on PATH, the host's
// `@skill-test/cli-<platform>-<arch>` package must be installed and carry
// bin/skilltest (bin/skilltest.exe on Windows), that binary must report
// `skilltest <expected-version>`, and the SDK's `validateSkill` — resolving the
// binary itself — must find <skill-dir> valid. A pass can only come from the
// bundled binary.
//
// Used by publish.yml's release-time install proof. Quiet on success: one line.
import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { createRequire } from "node:module";
import { delimiter, dirname, join } from "node:path";
import { pathToFileURL } from "node:url";

function fail(message) {
  console.error(`verify-bundled-npm: ${message}`);
  process.exit(1);
}

const [expectedVersion, skillDir] = process.argv.slice(2);
if (!expectedVersion || !skillDir) {
  fail("usage: verify-bundled-npm.mjs <expected-version> <skill-dir>");
}

delete process.env.SKILLTEST_BIN;
const exe = process.platform === "win32" ? "skilltest.exe" : "skilltest";
for (const dir of (process.env.PATH ?? "").split(delimiter)) {
  if (dir && existsSync(join(dir, exe))) {
    fail(`a skilltest is on PATH (${join(dir, exe)}), so a pass could not prove the bundle is used`);
  }
}

// Resolve from the consumer project, not from this script's directory.
const require = createRequire(join(process.cwd(), "package.json"));
const pkg = `@skill-test/cli-${process.platform}-${process.arch}`;
let bundled;
try {
  bundled = join(dirname(require.resolve(`${pkg}/package.json`)), "bin", exe);
} catch {
  fail(`${pkg} is not installed; @skill-test/sdk's optionalDependencies must pull it on this host`);
}
if (!existsSync(bundled)) fail(`${pkg} carries no ${bundled}`);

const version = execFileSync(bundled, ["--version"], { encoding: "utf8" }).trim();
if (version !== `skilltest ${expectedVersion}`) {
  fail(`${bundled} --version said '${version}', not 'skilltest ${expectedVersion}'`);
}

const sdk = await import(pathToFileURL(require.resolve("@skill-test/sdk")).href);
const report = await sdk.validateSkill(skillDir);
if (!report.valid) {
  fail(`validateSkill('${skillDir}') through the bundled CLI was not valid: ${JSON.stringify(report)}`);
}

console.log(`verify-bundled-npm: ${version} bundled at ${bundled}`);
