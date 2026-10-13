import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import { report } from "@onebudgetspec/sdk";
import { string, telemetry as validateTelemetry } from "./validation.mjs";
import { loadGraph, reconcileTelemetry, sourceDigest } from "./workflow-graph.mjs";
process.chdir(fileURLToPath(new URL("../", import.meta.url)));
try {
  const args = parseArgs({
    options: { metric: { type: "string" }, telemetry: { type: "string" } },
    strict: true,
  }).values;
  const metric = string(args.metric, "--metric");
  // A path passed from budgets.yaml is relative to .github; the public command uses the root.
  const path = string(args.telemetry, "--telemetry");
  const telemetry = validateTelemetry(
    JSON.parse(readFileSync(path.startsWith(".telemetry/") ? `.github/${path}` : path, "utf8")),
  );
  if (telemetry.schema_version !== 2 || telemetry.sourceDigest !== sourceDigest())
    throw new Error("missing or stale workflow telemetry; run github-workflows:test");
  reconcileTelemetry(telemetry, loadGraph());
  if (
    !Array.isArray(telemetry.runs) ||
    !Array.isArray(telemetry.lifecycles) ||
    !telemetry.runs.length ||
    !telemetry.lifecycles.length
  )
    throw new Error("incomplete workload telemetry; run github-workflows:test");
  let measured;
  if (metric === "jobs-per-run")
    measured = Math.max(
      ...telemetry.runs.map(
        (run) => run.jobs.filter((j) => j.requested && j.runner.startsWith("macos-")).length,
      ),
    );
  else if (metric === "runs-per-change")
    measured = Math.max(
      ...telemetry.lifecycles.map(
        (l) =>
          l.runs.filter((id) =>
            telemetry.runs
              .find((r) => r.id === id)
              ?.jobs.some((j) => j.requested && j.runner.startsWith("macos-")),
          ).length,
      ),
    );
  else throw new Error("unknown metric; use jobs-per-run or runs-per-change");
  const detail = JSON.stringify({
    scenarios: telemetry.scenarios,
    macRuns: telemetry.runs.filter((r) =>
      r.jobs.some((j) => j.requested && j.runner.startsWith("macos-")),
    ),
    lifecycles: telemetry.lifecycles,
  });
  report(measured, detail);
  console.log(JSON.stringify({ metric: metric, value: measured }));
} catch (error) {
  console.error(`macos-budget: ${error instanceof Error ? error.message : String(error)}`);
  process.exitCode = 1;
}
