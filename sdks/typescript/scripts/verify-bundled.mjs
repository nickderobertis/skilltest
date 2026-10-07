// Prove an installed `@skill-test/sdk` runs the CLI bundled in its platform package.
//
//   cd <consumer-project> && node <repo>/sdks/typescript/scripts/verify-bundled.mjs <expected-version> <skill-dir>
//
// Run it from a fresh project `@skill-test/sdk` was installed into. With
// $SKILLTEST_BIN unset and no `skilltest` on PATH, the host's
// `@skill-test/cli-<platform>-<arch>` package must be installed and carry
// bin/skilltest (bin/skilltest.exe on Windows), that binary must report
// `skilltest <expected-version>`, and the SDK's `validateSkill` — resolving the
// binary itself — must find <skill-dir> valid. A pass can only come from the
// bundled binary.
//
// Used by publish.yml's release-time install proof; not published (the package
// ships only dist/). Quiet on success: one line.
import { spawnSync } from "node:child_process";
import { existsSync, statSync } from "node:fs";
import { createRequire } from "node:module";
import { delimiter, dirname, join } from "node:path";
import { pathToFileURL } from "node:url";

function fail(message) {
  console.error(`verify-bundled: ${message}`);
  process.exit(1);
}

const [expectedVersion, skillDir, ...extra] = process.argv.slice(2);
let isDir = false;
if (skillDir && existsSync(skillDir)) {
  try {
    isDir = statSync(skillDir).isDirectory();
  } catch (error) {
    fail(
      `could not read ${skillDir} (${error}); check its permissions, or pass another skill directory`,
    );
  }
}
if (!expectedVersion || !skillDir || extra.length > 0 || !isDir) {
  fail(
    "usage: verify-bundled.mjs <expected-version> <skill-dir>; pass the release version and an existing skill directory",
  );
}

Reflect.deleteProperty(process.env, "SKILLTEST_BIN");
const exe = process.platform === "win32" ? "skilltest.exe" : "skilltest";
for (const dir of (process.env.PATH ?? "").split(delimiter)) {
  if (dir && existsSync(join(dir, exe))) {
    fail(
      `a skilltest is on PATH (${join(dir, exe)}), so a pass could not prove the bundle is used; drop that directory from PATH and rerun`,
    );
  }
}

// Resolve from the consumer project, not from this script's directory.
const require = createRequire(join(process.cwd(), "package.json"));
const pkg = `@skill-test/cli-${process.platform}-${process.arch}`;
let pkgJson;
try {
  pkgJson = require.resolve(`${pkg}/package.json`);
} catch (error) {
  fail(
    `${pkg} is not installed in ${process.cwd()} (${error}); run this from the consumer project and reinstall @skill-test/sdk there without --no-optional`,
  );
}
const bundled = join(dirname(pkgJson), "bin", exe);
if (!existsSync(bundled)) {
  fail(
    `${pkg} carries no ${bundled}; republish it with the binary staged by scripts/stage-npm-binary.sh`,
  );
}

const ran = spawnSync(bundled, ["--version"], { encoding: "utf8" });
if (ran.status !== 0) {
  fail(
    `${bundled} --version exited ${ran.status ?? ran.error} (${(ran.stderr ?? "").trim()}); the bundled binary does not run on this host, so rebuild ${pkg} for its target`,
  );
}
const version = ran.stdout.trim();
if (version !== `skilltest ${expectedVersion}`) {
  fail(
    `${bundled} --version said '${version}', not 'skilltest ${expectedVersion}'; install @skill-test/sdk@${expectedVersion}, or restage ${pkg} with that release's binary`,
  );
}

let sdk;
try {
  sdk = await import(pathToFileURL(require.resolve("@skill-test/sdk")).href);
} catch (error) {
  fail(
    `@skill-test/sdk does not load from ${process.cwd()} (${error}); install it there and rerun`,
  );
}
let report;
try {
  report = await sdk.validateSkill(skillDir);
} catch (error) {
  fail(
    `validateSkill('${skillDir}') through the bundled CLI threw (${error}); rerun it by hand from ${process.cwd()} to see the full error`,
  );
}
if (!report.valid) {
  fail(
    `validateSkill('${skillDir}') through the bundled CLI found ${JSON.stringify(report.findings)}; point this check at a valid skill such as tests/fixtures/smoke/greeter`,
  );
}

console.log(`verify-bundled: ${version} bundled at ${bundled}`);
