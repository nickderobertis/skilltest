// Usage: project-format.mjs [--scripts] [--write]. Checks (or rewrites) an
// annotated project.json with Biome's JSONC formatter.
// Exit codes: 0 formatted; 1 the file's format differs; 2 invalid invocation.
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { parseArgs } from "node:util";
let values;
try {
  ({ values } = parseArgs({
    options: { scripts: { type: "boolean" }, write: { type: "boolean" } },
    strict: true,
  }));
} catch (error) {
  console.error(
    `project-format: ${error instanceof Error ? error.message : String(error)}; usage: project-format.mjs [--scripts] [--write]`,
  );
  process.exit(2);
}
const target = values.scripts ? "scripts/project.json" : ".github/project.json";
const source = readFileSync(target, "utf8");
// Nx accepts JSONC; select Biome's JSONC parser for this one annotated project.
const formatted = execFileSync(
  "pnpm",
  [
    "exec",
    "biome",
    "format",
    "--config-path",
    ".github",
    "--stdin-file-path",
    ".github/project.jsonc",
  ],
  { input: source, encoding: "utf8" },
);
if (values.write) writeFileSync(target, formatted);
else if (source !== formatted) {
  console.error(
    `${target} format differs; run node .github/project-format.mjs${values.scripts ? " --scripts" : ""} --write`,
  );
  process.exitCode = 1;
}
