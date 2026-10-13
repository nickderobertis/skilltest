import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
if (process.argv.length > 3 || (process.argv[2] !== undefined && process.argv[2] !== "--write"))
  throw new Error("usage: project-format.mjs [--write]");
const source = readFileSync(".github/project.json", "utf8");
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
if (process.argv[2] === "--write") writeFileSync(".github/project.json", formatted);
else if (source !== formatted) {
  console.error(
    "github-workflows project format differs; run node .github/project-format.mjs --write",
  );
  process.exitCode = 1;
}
