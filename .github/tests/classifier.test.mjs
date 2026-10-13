import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { chmodSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";
import { classifyCLI, event, history } from "./history.mjs";
// llmlint: ignore[shell_test_tiers_stay_split] The task requires these deterministic real subprocess boundaries in the integrated workflow-recording test target; no external-service or model probe runs here.
test("real CLI verifies version-only histories and fails closed for forged/mixed/unreadable PRs", (t) => {
  const h = history(t);
  assert.deepEqual(classifyCLI(h, event()).isReleaseOnly, true);
  for (const login of ["github-actions[bot]", "nickderobertis", "someone"])
    assert.equal(classifyCLI(h, event({ sender: { login } })).macosRequired, false);
  for (const head of [h.dependency, h.source, "missing-ref"])
    assert.equal(classifyCLI(h, event(), head).macosRequired, true);
  for (const ref of ["feature", "release-plz", "release/0.2.0"]) {
    const e = event();
    e.pull_request.head.ref = ref;
    assert.equal(classifyCLI(h, e).macosRequired, true);
  }
  const fork = event();
  fork.pull_request.head.repo.full_name = "fork/skilltest";
  assert.equal(classifyCLI(h, fork).macosRequired, true);
  const bot = event({ sender: { login: "github-actions[bot]" } });
  bot.pull_request.head.ref = "feature";
  assert.equal(classifyCLI(h, bot).macosRequired, true);
  for (const malformed of ["{", "null", "[]", "{}"])
    assert.equal(classifyCLI(h, malformed).macosRequired, true);
  for (const name of [
    "push",
    "schedule",
    "workflow_run",
    "workflow_dispatch",
    "repository_dispatch",
    "release",
  ])
    assert.equal(classifyCLI(h, event({ event_name: name }), h.source).macosRequired, false);
  // An event name the policy does not know is the one non-PR case that requests Mac.
  for (const head of [h.release, h.source]) {
    const unknown = classifyCLI(h, event({ event_name: "merge_group" }), head);
    assert.equal(unknown.macosRequired, true);
    assert.equal(unknown.isReleaseOnly, false);
    assert.match(unknown.reason, /unrecognised event "merge_group"/);
  }
});

// llmlint: ignore[shell_test_tiers_stay_split] The task requires these real Git/classifier journeys inside the integrated deterministic recording target, without external-service or model probes.
test("a registry dependency sharing a workspace package's name is dependency content", (t) => {
  const h = history(t);
  const git = (...args) => execFileSync("git", args, { cwd: h.dir, encoding: "utf8" }).trim();
  const commit = (files, message) => {
    for (const [path, src] of Object.entries(files)) {
      mkdirSync(join(h.dir, path, ".."), { recursive: true });
      writeFileSync(join(h.dir, path), src);
    }
    git("add", ".");
    git("commit", "--quiet", "-m", message);
    return git("rev-parse", "HEAD");
  };
  const cases = {
    "Cargo.toml": (v) =>
      `[workspace.package]\nversion="0.1.0"\n[workspace.dependencies]\nskilltest-core={version="${v}"}\n`,
    "Cargo.lock": (v) =>
      `version=4\n[[package]]\nname="skilltest-cli"\nversion="0.1.0"\n[[package]]\nname="skilltest-core"\nversion="${v}"\nsource="registry+https://github.com/rust-lang/crates.io-index"\n`,
    "plugins/pytest/pyproject.toml": (v) =>
      `[project]\nname="skilltest-pytest"\nversion="0.1.0"\ndependencies=["skilltest-sdk==${v}"]\n`,
    "uv.lock": (v) =>
      `version=1\n[[package]]\nname="skilltest-pytest"\nversion="0.1.0"\nsource={editable="plugins/pytest"}\n[package.metadata]\nrequires-dist=[{name="skilltest-sdk",specifier="==${v}"}]\n[[package]]\nname="skilltest-sdk"\nversion="${v}"\nsource={registry="https://pypi.org/simple"}\n`,
  };
  for (const [path, render] of Object.entries(cases)) {
    git("checkout", "--quiet", h.release);
    h.base = commit({ [path]: render("0.1.0") }, `registry ${path}`);
    const bumped = commit({ [path]: render("0.2.0") }, `bump registry ${path}`);
    const result = classifyCLI(h, event(), bumped);
    assert.equal(result.macosRequired, true, path);
    assert.equal(result.isReleaseOnly, false, path);
  }
});

// llmlint: ignore[shell_test_tiers_stay_split] The task requires these real Git/classifier journeys inside the integrated deterministic recording target, without external-service or model probes.
test("parsed TOML and workspace lockfiles admit only internal version fields", (t) => {
  const h = history(t);
  const git = (...args) => execFileSync("git", args, { cwd: h.dir, encoding: "utf8" }).trim();
  git("checkout", "--quiet", h.base);
  const fixtures = {
    "Cargo.toml":
      '[workspace.package]\nversion="0.1.0"\n[workspace.dependencies]\nskilltest-core={path="crates/skilltest-core",version="0.1.0"}\nserde="1.0.0"\n',
    "Cargo.lock":
      'version=4\n[[package]]\nname="skilltest-core"\nversion="0.1.0"\n[[package]]\nname="serde"\nversion="1.0.0"\nsource="registry+https://example.invalid"\n',
    "sdks/python/pyproject.toml":
      '[project]\nname="skilltest-sdk"\nversion="0.1.0"\ndependencies=["pydantic>=2.7"]\n',
    "plugins/pytest/pyproject.toml":
      '[project]\nname="skilltest-pytest"\nversion="0.1.0"\ndependencies=["skilltest-sdk==0.1.0", "pytest>=8"]\n[tool.uv.sources]\nskilltest-sdk={workspace=true}\n',
    "uv.lock":
      'version=1\n[[package]]\nname="skilltest-sdk"\nversion="0.1.0"\nsource={editable="sdks/python"}\n[[package]]\nname="skilltest-pytest"\nversion="0.1.0"\nsource={editable="plugins/pytest"}\n[package.metadata]\nrequires-dist=[{name="skilltest-sdk",specifier="==0.1.0"},{name="pydantic",specifier=">=2.7"}]\n[[package]]\nname="pydantic"\nversion="1.0.0"\nsource={registry="https://example.invalid"}\n',
    "pnpm-lock.yaml": "lockfileVersion: '9.0'\nimporters: {}\npackages: {}\n",
  };
  for (const [path, src] of Object.entries(fixtures)) {
    mkdirSync(join(h.dir, path, ".."), { recursive: true });
    writeFileSync(join(h.dir, path), src);
  }
  git("add", ".");
  git("commit", "--quiet", "-m", "all manifests");
  h.base = git("rev-parse", "HEAD");
  for (const [path, src] of Object.entries(fixtures)) {
    writeFileSync(join(h.dir, path), src.replaceAll("0.1.0", "0.2.0"));
  }
  git("add", ".");
  git("commit", "--quiet", "-m", "version fields");
  const release = git("rev-parse", "HEAD");
  assert.equal(classifyCLI(h, event(), release).isReleaseOnly, true);
  git("checkout", "--quiet", release);
  writeFileSync(
    join(h.dir, "uv.lock"),
    fixtures["uv.lock"].replaceAll("0.1.0", "0.2.0").replace("==0.2.0", "==invalid"),
  );
  git("add", ".");
  git("commit", "--quiet", "-m", "invalid SDK metadata pin");
  const invalidPin = classifyCLI(h, event(), git("rev-parse", "HEAD"));
  assert.equal(invalidPin.macosRequired, true);
  assert.match(invalidPin.reason, /invalid SDK pin/);

  for (const [path, src] of Object.entries(fixtures)) {
    git("checkout", "--quiet", release);
    writeFileSync(
      join(h.dir, path),
      src
        .replaceAll("0.1.0", "0.2.0")
        .replaceAll("1.0.0", "2.0.0")
        .replaceAll(">=2.7", ">=3.0")
        .replaceAll("pytest>=8", "pytest>=9")
        .replace("packages: {}", "packages: {foreign: {integrity: changed}}"),
    );
    git("add", ".");
    const changed = git("diff", "--cached", "--name-only");
    if (changed) {
      git("commit", "--quiet", "-m", "dependency content");
      assert.equal(classifyCLI(h, event(), git("rev-parse", "HEAD")).macosRequired, true, path);
    }
  }
  git("checkout", "--quiet", release);
  chmodSync(join(h.dir, "CHANGELOG.md"), 0o755);
  git("add", ".");
  git("commit", "--quiet", "-m", "permissions");
  assert.equal(classifyCLI(h, event(), git("rev-parse", "HEAD")).macosRequired, true);
});

// llmlint: ignore[shell_test_tiers_stay_split] The task requires these real Git/classifier journeys inside the integrated deterministic recording target, without external-service or model probes.
test("classifier rejects empty and added/deleted/renamed release histories", (t) => {
  const h = history(t);
  const git = (...args) => execFileSync("git", args, { cwd: h.dir, encoding: "utf8" }).trim();
  assert.equal(classifyCLI(h, event(), h.base).macosRequired, true);
  git("checkout", "--quiet", h.release);
  writeFileSync(join(h.dir, "another-file"), "new");
  git("add", ".");
  git("commit", "--quiet", "-m", "added");
  assert.equal(classifyCLI(h, event(), git("rev-parse", "HEAD")).macosRequired, true);
  git("checkout", "--quiet", h.release);
  git("rm", "CHANGELOG.md");
  git("commit", "--quiet", "-m", "deleted");
  assert.equal(classifyCLI(h, event(), git("rev-parse", "HEAD")).macosRequired, true);
  git("checkout", "--quiet", h.release);
  git("mv", "CHANGELOG.md", "RENAMED.md");
  git("commit", "--quiet", "-m", "renamed");
  assert.equal(classifyCLI(h, event(), git("rev-parse", "HEAD")).macosRequired, true);
});
