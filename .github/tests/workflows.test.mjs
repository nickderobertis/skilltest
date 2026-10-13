import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import test from "node:test";
import YAML from "yaml";
import {
  loadGraph,
  matrixRows,
  resolveRun,
  scenarioRuns,
  sourceDigest,
} from "../workflow-graph.mjs";
import { classifyCLI, event, history } from "./history.mjs";

const graph = loadGraph();
const classes = [
  ["pr-opened", "pull_request", "opened"],
  ["pr-synchronize", "pull_request", "synchronize"],
  ["pr-reopened", "pull_request", "reopened"],
  ["base-push", "push"],
  ["release-pr-merge", "push"],
  ["release-tag", "push"],
  ["release-published", "release"],
  ["downstream", "workflow_run"],
  ["dispatch", "repository_dispatch"],
  ["schedule", "schedule"],
  ["manual-default", "workflow_dispatch"],
  ["manual-opt-in", "workflow_dispatch"],
];
function scenario(label, name, action, diff, ownership) {
  const e = event({
    label: `${label}/${diff}/${ownership}`,
    event_name: name,
    action,
    changed:
      diff === "source"
        ? ["crates/skilltest-core/src/runner.rs"]
        : diff === "dependency"
          ? ["Cargo.toml"]
          : ["CHANGELOG.md", "sdks/typescript/package.json"],
    ref: label === "release-tag" ? "refs/tags/v0.13.0" : "refs/heads/main",
    head_commit: {
      message: label === "release-pr-merge" ? "chore(release): 0.13.0" : "feat: ordinary change",
    },
    inputs: { macos_diagnostic: label === "manual-opt-in" },
    workflow_run: {
      name: "publish",
      id: 123,
      head_sha: "exact-head",
      head_branch: "v0.13.0",
      head_repository: { full_name: "nickderobertis/skilltest" },
      event: "push",
      conclusion: "success",
    },
  });
  if (diff === "source") e.pull_request.head.ref = "feature";
  if (ownership === "fork") e.pull_request.head.repo.full_name = "fork/skilltest";
  return e;
}
test("actual workflow graph across the complete workload records telemetry", (t) => {
  const h = history(t);
  const runs = [];
  const scenarios = [];
  const lifecycles = [];
  for (const diff of ["source", "version-only", "dependency"])
    for (const ownership of ["same-repository", "fork"]) {
      const byClass = {};
      for (const [label, name, action] of classes) {
        const e = scenario(label, name, action, diff, ownership);
        const classified = classifyCLI(
          h,
          e,
          diff === "source" ? h.source : diff === "dependency" ? h.dependency : h.release,
        );
        const resolved = scenarioRuns(graph, e, classified);
        scenarios.push({ label: e.label, classification: classified });
        byClass[label] = [];
        for (const run of resolved) {
          const id = `${e.label}:${run.workflow}`;
          runs.push({
            id,
            scenario: e.label,
            event: name,
            workflow: run.workflow,
            manual: label.startsWith("manual-"),
            jobs: run.rows,
          });
          byClass[label].push(id);
          const mac = run.rows.filter((j) => j.requested && j.runner.startsWith("macos-"));
          for (const row of mac) {
            assert.ok(
              (run.workflow === "ci.yml" &&
                name === "pull_request" &&
                classified.macosRequired &&
                row.job === "mac-check") ||
                (run.workflow === "publish.yml" &&
                  (label === "release-tag" || label === "manual-opt-in") &&
                  row.job === "darwin"),
              JSON.stringify(row),
            );
          }
          if (
            label === "manual-default" ||
            name === "schedule" ||
            label === "base-push" ||
            label === "release-pr-merge" ||
            name === "workflow_run"
          )
            assert.deepEqual(mac, []);
        }
        if (name === "pull_request") {
          const names = resolved.flatMap((r) =>
            r.rows.filter((j) => j.requested).map((j) => j.name),
          );
          for (const required of [
            "check (macos-latest)",
            "check (ubuntu-latest)",
            "llmlint",
            "pr-title",
          ])
            assert.equal(names.filter((n) => n === required).length, 1, `${e.label}: ${required}`);
          const ci = resolved.find((r) => r.workflow === "ci.yml");
          assert.equal(
            ci.results["mac-check"].result,
            classified.macosRequired ? "success" : "skipped",
          );
        }
        if (label === "release-tag") {
          const publish = resolved.find((r) => r.workflow === "publish.yml");
          assert.equal(publish.results.darwin.result, "success");
          assert.equal(publish.rows.filter((j) => j.job === "darwin").length, 1);
          assert.deepEqual(
            publish.rows
              .filter((j) => j.job === "binaries")
              .map((j) => j.target)
              .sort(),
            [
              "x86_64-unknown-linux-gnu",
              "aarch64-unknown-linux-gnu",
              "x86_64-pc-windows-msvc",
              "aarch64-pc-windows-msvc",
            ].sort(),
          );
          assert.equal(
            resolved.some((r) => r.workflow === "release.yml"),
            false,
            "archives wait for exact build artifacts",
          );
        }
      }
      for (let optional = 0; optional < 8; optional++)
        lifecycles.push({
          id: `${diff}/${ownership}/publish-${optional}`,
          optionalPublish: {
            crates: Boolean(optional & 1),
            pypi: Boolean(optional & 2),
            npm: Boolean(optional & 4),
          },
          runs: [
            "pr-synchronize",
            "base-push",
            "pr-opened",
            "release-pr-merge",
            "release-tag",
            "release-published",
            "downstream",
          ].flatMap((label) => (label === "pr-opened" ? [] : byClass[label])),
        });
    }
  // A source change's lifecycle contains its ordinary PR AND the subsequent automation release PR.
  for (const lifecycle of lifecycles) {
    const [diff, ownership] = lifecycle.id.split("/");
    const releasePR = runs
      .filter((r) => r.scenario === "pr-opened/version-only/same-repository")
      .map((r) => r.id);
    lifecycle.runs.push(...releasePR);
    assert.ok(lifecycle.runs.length > 0, `${diff}/${ownership}`);
  }
  mkdirSync(".github/.telemetry", { recursive: true });
  writeFileSync(
    ".github/.telemetry/macos-policy.json",
    `${JSON.stringify(
      { schema_version: 1, sourceDigest: sourceDigest(), scenarios, runs, lifecycles },
      null,
      2,
    )}\n`,
  );
});

test("base master pushes and automatic diagnostic attempts never spend Mac", () => {
  for (const event_name of ["push", "workflow_run", "schedule", "repository_dispatch", "release"]) {
    const e = scenario("base-push", event_name, undefined, "source", "same-repository");
    e.ref = "refs/heads/master";
    e.inputs.macos_diagnostic = true;
    assert.equal(
      resolveRun(graph, "publish.yml", e, { macosRequired: false }).results.darwin.result,
      event_name === "push" ? "success" : "skipped",
    );
    const runs = scenarioRuns(graph, e, { macosRequired: false });
    assert.deepEqual(
      runs.flatMap((r) => r.rows).filter((j) => j.requested && j.runner.startsWith("macos-")),
      [],
    );
  }
});

test("required carrier command propagates third-party results and distinguishes authorized omission", () => {
  const ci = graph["ci.yml"];
  const carrier = ci.jobs["mac-result"];
  assert.equal(carrier.name, "check (macos-latest)");
  assert.match(carrier.if, /always\(\)/);
  assert.deepEqual(carrier.needs, ["policy", "mac-check"]);
  const command = carrier.steps.find((s) => s.run)?.run;
  assert.equal(command, "node .github/required-result.mjs");
  for (const policy of ["success", "failure", "cancelled", "skipped", undefined])
    for (const mac of ["success", "failure", "cancelled", "skipped", undefined])
      for (const required of ["true", "false", "", undefined]) {
        const needs = { policy: { result: policy }, "mac-check": { result: mac } };
        const result = spawnSync("bash", ["-c", command], {
          encoding: "utf8",
          env: { ...process.env, NEEDS: JSON.stringify(needs), MACOS_REQUIRED: required ?? "" },
        });
        const allowed =
          policy === "success" &&
          ((required === "true" && mac === "success") ||
            (required === "false" && mac === "skipped"));
        assert.equal(
          result.status === 0,
          allowed,
          JSON.stringify({ policy, mac, required, stdout: result.stdout, stderr: result.stderr }),
        );
        if (allowed && required === "false") assert.match(result.stdout, /no Mac tests ran/);
      }
  const e = scenario("pr-synchronize", "pull_request", "synchronize", "source", "same-repository");
  for (const outcome of ["failure", "cancelled", "skipped"]) {
    const resolved = resolveRun(
      graph,
      "ci.yml",
      e,
      { macosRequired: true },
      { "ci.yml:policy": outcome },
    );
    assert.equal(resolved.results["mac-check"].result, "success");
    assert.equal(resolved.rows.find((j) => j.job === "mac-result").requested, true);
    assert.equal(resolved.results["mac-result"].result, "failure");
  }
  for (const outcome of ["failure", "cancelled", "skipped"]) {
    const resolved = resolveRun(
      graph,
      "ci.yml",
      e,
      { macosRequired: true },
      { "ci.yml:mac-check": outcome },
    );
    assert.equal(resolved.results["mac-result"].result, "failure");
  }
  assert.equal(
    resolveRun(graph, "ci.yml", e, { macosRequired: false }).results["mac-result"].result,
    "success",
  );
});

test("routing resolver rejects unsupported calls, runners, needs and matrix inputs", () => {
  const e = scenario("pr-opened", "pull_request", "opened", "source", "same-repository");
  for (const mutation of [
    (g) => {
      g["ci.yml"].jobs["mac-check"]["runs-on"] = "${{ vars.unknown }}";
    },
    (g) => {
      g["ci.yml"].jobs["mac-check"].uses = "other/repo/workflow.yml@v1";
    },
    (g) => {
      g["ci.yml"].jobs["mac-check"].needs = ["missing"];
    },
    (g) => {
      g["ci.yml"].jobs["mac-check"].strategy = { matrix: { os: "${{ vars.missing }}" } };
    },
  ]) {
    const broken = structuredClone(graph);
    mutation(broken);
    assert.throws(() => resolveRun(broken, "ci.yml", e, { macosRequired: true }), /unresolved/);
  }
  assert.deepEqual(
    matrixRows({
      os: ["linux", "mac"],
      arch: ["a", "b"],
      exclude: [{ os: "mac", arch: "a" }],
      include: [
        { os: "linux", extra: 1 },
        { os: "win", arch: "a" },
      ],
    }),
    [
      { os: "linux", arch: "a", extra: 1 },
      { os: "linux", arch: "b", extra: 1 },
      { os: "mac", arch: "b" },
      { os: "win", arch: "a" },
    ],
  );
});

test("release artifact authority retains every platform and enforces pre-publication Darwin proofs", () => {
  const platforms = JSON.parse(
    execFileSync(
      "python3",
      [
        "-c",
        "import json,tomllib;print(json.dumps(tomllib.load(open('release-platforms.toml','rb'))['platform']))",
      ],
      { encoding: "utf8" },
    ),
  );
  // The Linux native journey executes these helpers; Mac execution belongs to
  // the existing native CI lane. Preserve both original bundle assertions here.
  const ciBundle = graph["ci.yml"].jobs["mac-check"].steps.map((s) => s.run ?? "").join("\n");
  assert.match(ciBundle, /scripts\/smoke-python-bundle.sh/);
  assert.match(ciBundle, /scripts\/smoke-npm-bundle.sh/);
  const publish = graph["publish.yml"].jobs;
  // Linux cannot execute Darwin/Rosetta. Native packaging is exercised on the
  // host through release-packaging:packaging; these assertions preserve the
  // Mac job's runtime commands for the actual Apple Silicon runner to execute.
  const darwin = publish.darwin.steps.flatMap((s) => (s.run ? [s.run] : [])).join("\n");
  for (const p of platforms) {
    if (p.target.includes("apple")) assert.ok(darwin.includes(p.target));
    else
      assert.ok(
        publish.binaries.strategy.matrix.include.some(
          (r) => r.target === p.target && r.os === p.runner && r.bin === p.bin,
        ),
      );
    assert.ok(graph["release.yml"].jobs.upload.steps.find((s) => s.run)?.run.includes(p.target));
  }
  for (const script of [
    "stage-release-product.sh",
    "verify-release-archive.sh",
    "smoke-python-bundle.sh",
    "smoke-npm-bundle.sh",
  ])
    assert.ok(darwin.includes(script));
  assert.match(darwin, /arch -x86_64 bash scripts\/smoke-python-bundle/);
  assert.match(darwin, /arch -x86_64 bash scripts\/smoke-npm-bundle/);
  assert.ok(publish.darwin.steps.some((s) => s.with?.architecture === "x64"));
  assert.deepEqual(publish.products.needs, ["binaries", "darwin"]);
  for (const job of ["crates", "pypi", "npm"]) {
    assert.equal(publish[job].needs, "products");
    assert.equal(publish[job]["runs-on"], "ubuntu-latest");
  }
  const release = graph["release.yml"];
  assert.deepEqual(release.on.workflow_run.workflows, ["publish"]);
  assert.equal(release.jobs.upload["runs-on"], "ubuntu-latest");
  const download = release.jobs.upload.steps.find((s) => s.uses === "actions/download-artifact@v4");
  assert.equal(download.with["run-id"], "${{ github.event.workflow_run.id }}");
  assert.equal(release.jobs.upload.steps[0].with.ref, "${{ github.event.workflow_run.head_sha }}");
  for (const invalid of [
    { event: "workflow_dispatch" },
    { head_repository: { full_name: "fork/skilltest" } },
    { head_branch: "main" },
    { head_branch: "v0.0.0" },
    { conclusion: "cancelled" },
  ]) {
    const e = scenario("downstream", "workflow_run", undefined, "source", "same-repository");
    Object.assign(e.workflow_run, invalid);
    assert.equal(
      resolveRun(graph, "release.yml", e, { macosRequired: false }).results.upload.result,
      "skipped",
    );
  }
});

test("budget reporters consume recorded telemetry and refuse stale/missing input", () => {
  for (const metric of ["jobs-per-run", "runs-per-change"]) {
    const result = spawnSync(
      process.execPath,
      [
        ".github/macos-budget.mjs",
        "--metric",
        metric,
        "--telemetry",
        ".github/.telemetry/macos-policy.json",
      ],
      { encoding: "utf8" },
    );
    assert.equal(result.status, 0, result.stderr);
    assert.equal(typeof JSON.parse(result.stdout).value, "number");
  }
  const missing = spawnSync(
    process.execPath,
    [
      ".github/macos-budget.mjs",
      "--metric",
      "jobs-per-run",
      "--telemetry",
      ".github/.telemetry/missing.json",
    ],
    { encoding: "utf8" },
  );
  assert.notEqual(missing.status, 0);
  writeFileSync(
    ".github/.telemetry/stale.json",
    JSON.stringify({
      ...JSON.parse(readFileSync(".github/.telemetry/macos-policy.json", "utf8")),
      sourceDigest: "stale",
    }),
  );
  const stale = spawnSync(
    process.execPath,
    [
      ".github/macos-budget.mjs",
      "--metric",
      "jobs-per-run",
      "--telemetry",
      ".github/.telemetry/stale.json",
    ],
    { encoding: "utf8" },
  );
  assert.notEqual(stale.status, 0);
  assert.match(stale.stderr, /stale/);
});

test("release tag/version/exact-head checks drive the production command", (t) => {
  const h = history(t);
  const run = (tag, version, head, tagHead) =>
    spawnSync("bash", ["scripts/check-release-identity.sh", tag, version, head, tagHead], {
      encoding: "utf8",
    });
  assert.equal(run("v0.2.0", "0.2.0", h.release, h.release).status, 0);
  for (const [tag, version, head, tagHead] of [
    ["v0.2.0", "0.1.0", h.release, h.release],
    ["v0.2.0", "0.2.0", h.source, h.release],
    ["v0.2.0", "0.2.0", "unknown", h.release],
  ]) {
    const result = run(tag, version, head, tagHead);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /exact commit disagree/);
  }
  assert.ok(
    graph["publish.yml"].jobs.darwin.steps.some((s) =>
      s.run?.includes("scripts/check-release-identity.sh"),
    ),
  );
  assert.ok(
    graph["release.yml"].jobs.upload.steps.some((s) =>
      s.run?.includes("scripts/check-release-identity.sh"),
    ),
  );
});

test("forged and unreadable PRs drive the actual graph into required Mac coverage", (t) => {
  const h = history(t);
  for (const input of [
    "{",
    "null",
    event({
      pull_request: {
        title: "chore(release): 0.2.0",
        head: { ref: "forged-release", repo: { full_name: "nickderobertis/skilltest" } },
      },
    }),
  ]) {
    const classified = classifyCLI(h, input);
    const e = scenario(
      "pr-synchronize",
      "pull_request",
      "synchronize",
      "version-only",
      "same-repository",
    );
    const runs = scenarioRuns(graph, e, classified);
    assert.equal(runs.find((r) => r.workflow === "ci.yml").results["mac-check"].result, "success");
    for (const name of ["check (macos-latest)", "check (ubuntu-latest)", "llmlint", "pr-title"])
      assert.ok(runs.flatMap((r) => r.rows).some((j) => j.name === name && j.requested));
  }
});

test("SDK budget analysis alone judges an induced matrix expansion", () => {
  const original = JSON.parse(readFileSync(".github/.telemetry/macos-policy.json", "utf8"));
  const mutated = structuredClone(graph);
  const platforms = JSON.parse(
    execFileSync(
      "python3",
      [
        "-c",
        "import json,tomllib;print(json.dumps(tomllib.load(open('release-platforms.toml','rb'))['platform']))",
      ],
      { encoding: "utf8" },
    ),
  );
  mutated["ci.yml"].jobs["mac-check"].strategy = {
    matrix: { target: platforms.map((p) => p.target) },
  };
  const e = scenario("pr-synchronize", "pull_request", "synchronize", "source", "same-repository");
  const run = resolveRun(mutated, "ci.yml", e, { macosRequired: true });
  const induced = structuredClone(original);
  induced.runs.find((r) => r.id === "pr-synchronize/source/same-repository:ci.yml").jobs = run.rows;
  writeFileSync(".github/.telemetry/induced-matrix.json", JSON.stringify(induced));
  const registered = YAML.parse(readFileSync(".github/budgets.yaml", "utf8"));
  for (const budget of registered.budgets)
    budget.command = [
      process.execPath,
      "../macos-budget.mjs",
      "--metric",
      budget.command[3],
      "--telemetry",
      ".github/.telemetry/induced-matrix.json",
    ];
  writeFileSync(".github/.telemetry/induced-budgets.yaml", YAML.stringify(registered));
  const result = spawnSync(
    process.execPath,
    [".github/check-budgets.mjs", ".github/.telemetry/induced-budgets.yaml"],
    { encoding: "utf8" },
  );
  assert.equal(result.status, 1, result.stderr);
  assert.match(result.stdout, /jobs-per-run: over/);
  const within = spawnSync(process.execPath, [".github/check-budgets.mjs"], { encoding: "utf8" });
  assert.equal(within.status, 0, within.stderr);
  assert.match(within.stdout, /within/);
  const malformed = structuredClone(original);
  malformed.lifecycles[0].runs.push("missing-run");
  writeFileSync(".github/.telemetry/malformed.json", JSON.stringify(malformed));
  const invalid = spawnSync(
    process.execPath,
    [
      ".github/macos-budget.mjs",
      "--metric",
      "jobs-per-run",
      "--telemetry",
      ".github/.telemetry/malformed.json",
    ],
    { encoding: "utf8" },
  );
  assert.notEqual(invalid.status, 0);
  assert.match(invalid.stderr, /unresolved lifecycle run/);
});
