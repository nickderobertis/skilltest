import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
const target = process.argv[2] === "--scripts" ? "scripts/project.json" : ".github/project.json";
const args = process.argv.slice(2).filter((arg) => arg !== "--scripts");
if (args.length > 1 || (args[0] !== undefined && args[0] !== "--write"))
  throw new Error("usage: project-format.mjs [--write]");
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
if (args[0] === "--write") writeFileSync(target, formatted);
else if (source !== formatted) {
  console.error(
    "github-workflows project format differs; run node .github/project-format.mjs --write",
  );
  process.exitCode = 1;
}
