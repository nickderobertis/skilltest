import { execFileSync } from "node:child_process";
import { appendFileSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { isDeepStrictEqual, parseArgs } from "node:util";
import YAML from "yaml";
import { object, string } from "./validation.mjs";

const policy = object(
  JSON.parse(readFileSync(new URL("./macos-policy.json", import.meta.url), "utf8")),
  "policy",
);
if (
  typeof policy.releaseBranchPrefix !== "string" ||
  !policy.releaseBranchPrefix.startsWith("release-plz-")
)
  throw new Error("invalid release branch policy");
const version = /^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/;
const toml = new Set([
  "Cargo.toml",
  "Cargo.lock",
  "uv.lock",
  "sdks/python/pyproject.toml",
  "plugins/pytest/pyproject.toml",
]);
const jsonManifest =
  /^(?:sdks\/typescript(?:\/platforms\/cli-[\w-]+)?|plugins\/vitest)\/package\.json$/;
/** @param {string} path @param {string} content @returns {unknown} */
function semantic(path, content) {
  if (toml.has(path))
    return JSON.parse(
      execFileSync(
        "python3",
        [fileURLToPath(new URL("./semantic-version-diff.py", import.meta.url)), path],
        { input: content, encoding: "utf8" },
      ),
    );
  if (jsonManifest.test(path)) {
    const doc = object(JSON.parse(content), path);
    if (!version.test(string(doc.version, "manifest version")))
      throw new Error(`invalid version in ${path}`);
    doc.version = "<version>";
    return doc;
  }
  if (path === "pnpm-lock.yaml") {
    const lock = object(YAML.parse(content), "pnpm lock");
    if (lock.lockfileVersion !== "9.0") throw new Error("unsupported pnpm lockfile version");
    object(lock.importers, "pnpm importers");
    if (lock.packages !== undefined) object(lock.packages, "pnpm packages");
    if (lock.snapshots !== undefined) object(lock.snapshots, "pnpm snapshots");
    return lock;
  }
  throw new Error(`non-release file ${path}`);
}
/** @param {unknown} event @param {string|undefined} base @param {string|undefined} head @param {string} cwd @returns {import('./models.js').Classification} */
export function classify(event, base, head, cwd = process.cwd()) {
  // Missing event metadata is an unknown PR, never an authorized omission.
  const eventDoc =
    event && typeof event === "object" && !Array.isArray(event) ? object(event, "event") : {};
  const name = eventDoc.event_name;
  const knownEvents = new Set([
    "push",
    "pull_request",
    "pull_request_target",
    "workflow_dispatch",
    "workflow_run",
    "repository_dispatch",
    "schedule",
    "release",
  ]);
  if (typeof name === "string" && knownEvents.has(name) && name !== "pull_request")
    return { isReleaseOnly: false, macosRequired: false, reason: `${name}: no automatic Mac lane` };
  /** @param {string} reason */
  const required = (reason) => ({ isReleaseOnly: false, macosRequired: true, reason });
  try {
    if (name !== "pull_request") return required("unknown event metadata; PR coverage required");
    const pr = object(eventDoc.pull_request, "pull_request");
    const repository = object(eventDoc.repository, "repository");
    const prHead = object(pr.head, "PR head");
    const prRepo = object(prHead.repo, "PR head repository");
    const repositoryName = string(repository.full_name, "repository full_name");
    const branch = string(prHead.ref, "PR branch");
    if (
      prRepo.full_name !== repositoryName ||
      !branch.startsWith(string(policy.releaseBranchPrefix, "releaseBranchPrefix"))
    )
      return required("ordinary or fork PR");
    /** @param {string[]} args */
    const git = (args) =>
      execFileSync("git", args, { cwd, encoding: "utf8", stdio: ["pipe", "pipe", "pipe"] });
    const baseSha = git(["rev-parse", "--verify", "--end-of-options", `${base}^{commit}`]).trim();
    const headSha = git(["rev-parse", "--verify", "--end-of-options", `${head}^{commit}`]).trim();
    if (git(["diff", "--summary", baseSha, headSha]).includes("mode change"))
      return required("release diff changes file permissions");
    const names = git(["diff", "--name-status", "-z", baseSha, headSha])
      .split("\0")
      .filter(Boolean);
    if (!names.length) return required("empty release diff");
    for (let i = 0; i < names.length; i += 2) {
      const [status, path] = names.slice(i, i + 2);
      if (status !== "M") return required("release diff adds, removes or renames a file");
      if (path === "CHANGELOG.md") continue;
      const before = semantic(path, git(["show", `${baseSha}:${path}`]));
      const after = semantic(path, git(["show", `${headSha}:${path}`]));
      if (!isDeepStrictEqual(before, after))
        return required(`non-version fields changed in ${path}`);
    }
    return {
      isReleaseOnly: true,
      macosRequired: false,
      reason: "same-repository release-plz branch; parsed version/changelog-only diff",
    };
  } catch (error) {
    return required(
      `unreadable release classification: ${error instanceof Error ? error.message : String(error)}`,
    );
  }
}
function mainCLI() {
  if (process.argv[2] !== "classify")
    throw new Error("usage: macos-policy.mjs classify --event PATH --base REF --head REF");
  const parsed = parseArgs({
    args: process.argv.slice(3),
    options: { event: { type: "string" }, base: { type: "string" }, head: { type: "string" } },
    tokens: true,
    strict: true,
  });
  const seen = new Set();
  for (const token of parsed.tokens)
    if (token.kind === "option") {
      if (seen.has(token.name))
        throw new Error(`duplicate --${token.name}; pass one value per classifier flag`);
      seen.add(token.name);
    }
  const args = parsed.values;
  string(args.event, "--event");
  string(args.base, "--base");
  string(args.head, "--head");
  let event;
  try {
    event = JSON.parse(readFileSync(string(args.event, "--event"), "utf8"));
  } catch {
    event = { event_name: process.env.GITHUB_EVENT_NAME };
  }
  if (!event || Array.isArray(event) || typeof event !== "object") event = {};
  event.event_name ??= process.env.GITHUB_EVENT_NAME;
  const result = classify(event, args.base, args.head);
  console.log(JSON.stringify(result));
  if (process.env.GITHUB_OUTPUT)
    appendFileSync(
      process.env.GITHUB_OUTPUT,
      `macosRequired=${result.macosRequired}\nisReleaseOnly=${result.isReleaseOnly}\n`,
    );
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    mainCLI();
  } catch (error) {
    console.error(
      `macos-policy: ${error instanceof Error ? error.message : String(error)}; pass classify --event PATH --base REF --head REF`,
    );
    process.exitCode = 1;
  }
}
