import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

const scratch = process.env.ONEPIPELINE_NODE_SCRATCH_DIR ?? process.env.TMPDIR ?? "/tmp";
const script = resolve(".github/macos-policy.mjs");
export function history(t) {
  const dir = mkdtempSync(join(scratch, "macos-classifier-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const git = (...args) => execFileSync("git", args, { cwd: dir, encoding: "utf8" }).trim();
  git("init", "--quiet");
  git("config", "user.email", "test@example.invalid");
  git("config", "user.name", "Fixture");
  mkdirSync(join(dir, "sdks/typescript"), { recursive: true });
  writeFileSync(
    join(dir, "sdks/typescript/package.json"),
    JSON.stringify({
      name: "@skill-test/sdk",
      version: "0.1.0",
      dependencies: { example: "1.0.0" },
    }),
  );
  writeFileSync(join(dir, "CHANGELOG.md"), "# Changes\n");
  writeFileSync(join(dir, "source.rs"), "fn main() {}\n");
  git("add", ".");
  git("commit", "--quiet", "-m", "base");
  const base = git("rev-parse", "HEAD");
  writeFileSync(
    join(dir, "sdks/typescript/package.json"),
    JSON.stringify({
      name: "@skill-test/sdk",
      version: "0.2.0",
      dependencies: { example: "1.0.0" },
    }),
  );
  writeFileSync(join(dir, "CHANGELOG.md"), "# Changes\n0.2.0\n");
  git("add", ".");
  git("commit", "--quiet", "-m", "release");
  const release = git("rev-parse", "HEAD");
  writeFileSync(
    join(dir, "sdks/typescript/package.json"),
    JSON.stringify({
      name: "@skill-test/sdk",
      version: "0.2.0",
      dependencies: { example: "2.0.0" },
    }),
  );
  git("add", ".");
  git("commit", "--quiet", "-m", "dependency");
  const dependency = git("rev-parse", "HEAD");
  writeFileSync(join(dir, "source.rs"), 'fn main() { panic!("changed"); }\n');
  git("add", ".");
  git("commit", "--quiet", "-m", "source");
  return { dir, base, release, dependency, source: git("rev-parse", "HEAD") };
}
export function event(overrides = {}) {
  return {
    event_name: "pull_request",
    action: "synchronize",
    repository: { full_name: "nickderobertis/skilltest" },
    sender: { login: "nickderobertis" },
    pull_request: {
      title: "chore: release",
      head: { ref: "release-plz-2026-10-12", repo: { full_name: "nickderobertis/skilltest" } },
      base: { ref: "main" },
    },
    ...overrides,
  };
}
export function classifyCLI(h, eventDoc, head = h.release) {
  const file = join(h.dir, ".git", "skilltest-event.json");
  writeFileSync(file, typeof eventDoc === "string" ? eventDoc : JSON.stringify(eventDoc));
  return JSON.parse(
    execFileSync(
      process.execPath,
      [script, "classify", "--event", file, "--base", h.base, "--head", head],
      { cwd: h.dir, encoding: "utf8" },
    ),
  );
}
