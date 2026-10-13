// Usage: scheduling-description.mjs [BASE-REF] [OBSERVATION.json]. Prints the
// change request's before/after Mac scheduling description: BASE-REF (default
// origin/main) is the baseline read with git; the optional observation file
// holds the draft's exact-head runs. Exit codes: 0 printed; 1 unreadable input.
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import YAML from "yaml";
import {
  array,
  object,
  string,
  observation as validateObservation,
  workflow as validateWorkflow,
} from "./validation.mjs";
import { loadGraph, scenarioRuns } from "./workflow-graph.mjs";
const base = process.argv[2] ?? "origin/main";
const after = loadGraph();
const before = Object.fromEntries(
  Object.entries(after).map(([name, workflow]) => [
    name,
    name.endsWith(".yml") && !name.includes("/")
      ? validateWorkflow(
          YAML.parse(
            execFileSync("git", ["show", `${base}:.github/workflows/${name}`], {
              encoding: "utf8",
            }),
          ),
        )
      : workflow,
  ]),
);
const repo = { full_name: "nickderobertis/skilltest" };
const events = [
  {
    event_name: "pull_request",
    action: "synchronize",
    ref: "refs/pull/1/merge",
    pull_request: { head: { repo, ref: "feature" }, base: { ref: "main" } },
  },
  { event_name: "push", ref: "refs/heads/main", head_commit: { message: "feat: change" } },
  { event_name: "push", ref: "refs/tags/v0.13.0" },
  {
    event_name: "workflow_run",
    ref: "refs/heads/main",
    action: "completed",
    workflow_run: {
      name: "publish",
      event: "push",
      head_branch: "v0.13.0",
      head_sha: "exact-head",
      head_repository: repo,
      conclusion: "success",
    },
  },
  { event_name: "schedule", ref: "refs/heads/main" },
  { event_name: "workflow_dispatch", ref: "refs/heads/main", inputs: { macos_diagnostic: false } },
  { event_name: "workflow_dispatch", ref: "refs/heads/main", inputs: { macos_diagnostic: true } },
].map((e) => ({ ...e, repository: repo, changed: ["crates/skilltest-core/src/runner.rs"] }));
/** @param {import('./models.js').Graph} graph */
function maxCounts(graph) {
  /** @type {Record<string,number>} */
  const counts = {};
  for (const e of events)
    for (const run of scenarioRuns(graph, e, { macosRequired: e.event_name === "pull_request" })) {
      counts[run.workflow] = Math.max(
        counts[run.workflow] ?? 0,
        run.rows.filter((j) => j.requested && j.runner.startsWith("macos-")).length,
      );
    }
  return counts;
}
/** @param {import('./models.js').Workflow} workflow */
function triggers(workflow) {
  return Object.entries(workflow.on)
    .map(
      ([name, filter]) =>
        `${name}${filter?.branches ? ` (${filter.branches.join(", ")})` : filter?.tags ? ` (${filter.tags.join(", ")})` : filter?.workflows ? ` (${filter.workflows.join(", ")}: ${(filter.types ?? []).join(", ")})` : ""}`,
    )
    .join("; ");
}
const original = maxCounts(before);
const final = maxCounts(after);
console.log(
  "Consolidate Mac proofs into ordinary PR CI and one pre-publication Darwin build. Linux result carriers preserve required names and distinguish tested success from authorized omission. All registry publication and exact-build archive transport run on Linux.\n",
);
console.log(
  "**Intel macOS artifacts are discontinued from the next release.** macOS releases ship Apple Silicon (arm64) only: no Intel macOS archive, Python wheel or npm carrier is built or published, and scripts/install.sh refuses Intel macOS with a message saying it is no longer supported (an Apple Silicon Mac in a Rosetta shell still gets the arm64 binary). Every other released platform and artifact is unchanged. Installers for third-party developer tools (scripts/install-oneharness.sh, .github/setup-screencomp.sh, the SDK's oneharness carrier map) keep their Intel macOS entries, since those projects still publish Intel builds; the manager approved leaving them.\n",
);
console.log(
  `Workflow counts below are contract-level evaluations of baseline ${base} and this tree, including matrices and reusable calls. They are not observed release executions.\n`,
);
console.log(
  "| Workflow | Before: Mac jobs/run; triggers | After: Mac jobs/run; triggers |\n| --- | --- | --- |",
);
for (const name of Object.keys(after).filter((name) => (original[name] ?? 0) || (final[name] ?? 0)))
  console.log(
    `| ${name} | ${original[name] ?? 0}; ${triggers(before[name])} | ${final[name] ?? 0}; ${triggers(after[name])} |`,
  );
console.log(
  "\nCI: ordinary PR 1, semantic release-only PR 0, base push 0. Publish: release-tag 1; manual default 0; explicit diagnostic 1, outside the lifecycle and never publishing. Release and automatic downstream/schedule runs request 0 Mac runners. The external screencomp reusable workflow remains Linux-only and is traversed from its locked upstream bytes.\n",
);
console.log(
  "Retained release platforms and forms (read from release-platforms.toml):\n\n| Target | Archive | Python wheel platform | npm carrier |\n| --- | --- | --- | --- |",
);
const platforms = array(
  JSON.parse(
    execFileSync(
      "python3",
      [
        "-c",
        "import json,tomllib;print(json.dumps(tomllib.load(open('release-platforms.toml','rb'))['platform']))",
      ],
      { encoding: "utf8" },
    ),
  ),
  "platforms",
).map((raw) => {
  const platform = object(raw, "release platform");
  return {
    target: string(platform.target, "target"),
    bin: string(platform.bin, "bin"),
    wheel_tag: string(platform.wheel_tag, "wheel_tag"),
    npm_package: string(platform.npm_package, "npm_package"),
  };
});
for (const p of platforms)
  console.log(
    `| ${p.target} | skilltest-${p.target}.${p.bin.endsWith(".exe") ? "zip" : "tar.gz"} + sha256 | ${p.wheel_tag} | ${p.npm_package} |`,
  );
console.log(
  "\nRetained: skilltest-core and skilltest-cli crates; skilltest-sdk platform/pure wheels and sdist; skilltest-pytest; @skill-test/sdk and @skill-test/vitest. Semantic-release and set-version retain lockstep synchronization. Each target has one compiled input for archive/wheel/npm, with payload hashes verified before publication. The one Apple Silicon Mac job builds, stages and installs the arm64 products natively, with no Rosetta or x86_64 cross-compilation.\n",
);
console.log(
  "Local proof: github-workflows:test emits ignored digest-bound telemetry for 12 event classes × 3 diff classes × 2 ownership classes and all optional-publication combinations. Numeric verdicts are produced only by onebudgetspec. The native production packaging journey builds, packs, installs and runs host-native products and exercises missing-provider/corrupt-wheel failure paths; foreign platform headers are packaging-contract evidence only.\n",
);
console.log(
  "The budgets_track_product_owner_outcomes ignore marker was omitted because that rule is not configured in this repository. Both budget descriptions carry the manager-approved separate scheduling-constraints exception; the jobs/run description also records its inner_measure_reason. llmlint configuration and plugins are unchanged.\n",
);
console.log(
  "Scoped lint exceptions: the site-specific upstream comment annotations in .github/tests/fixtures/screencomp-v0.4.2.yml preserve immutable upstream bytes, verified after removal of exactly those annotations. github-workflows:test carries shell_test_tiers_stay_split because its recording contract requires the deterministic real Git/classifier/carrier boundaries in the integrated graph suite. release-packaging:packaging carries new_projects_keep_the_graph_uniform because native builder journeys stay behind the explicit release edge, with ordinary cheap lint/format targets retained. The standing agents_md_durable_and_terse block in AGENTS.md still covers its composition audit; the changed Mac-proof location remains part of that audit. No llmlint configuration or plugins changed.\n",
);
console.log(
  "Manager-approved narrow adjustment to the Linux-only visual-docs caller: screencomp-version is pinned to 0.4.2 so its production architecture reader matches the checksum-pinned reader used by the graph contract. Its events, runners and assertions are unchanged.\n",
);
if (process.argv[3]) {
  const observed = validateObservation(JSON.parse(readFileSync(process.argv[3], "utf8")));
  console.log(`Observed draft exact-head runs for ${observed.head}:\n`);
  for (const run of observed.runs)
    console.log(
      `- [${run.name}](${run.html_url}): ${run.status}; ${run.conclusion ?? "no conclusion yet"}. ${run.jobs.map((j) => `${j.name}: ${j.status}/${j.conclusion ?? "pending"}`).join("; ")}`,
    );
  console.log(
    "\nQueued, in-progress and skipped states above are observations, not passing-test claims. Release routes remain locally modeled; no release was cut.",
  );
} else
  console.log(
    "Draft exact-head observations: draft has not yet been published; no hosted-run result is claimed.",
  );
