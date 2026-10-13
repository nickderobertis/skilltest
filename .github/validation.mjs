/** @param {unknown} value @param {string} where @returns {Record<string,unknown>} */
export function object(value, where) {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw new Error(`${where}: expected an object`);
  // The runtime checks reject null, arrays and primitives; this assertion only
  // supplies the index signature TypeScript cannot infer from typeof object.
  return /** @type {Record<string,unknown>} */ (value);
}
/** @param {unknown} value @param {string} where */
export function string(value, where) {
  if (typeof value !== "string" || !value) throw new Error(`${where}: expected a nonempty string`);
  return value;
}
/** @param {unknown} value @param {string} where @returns {unknown[]} */
export function array(value, where) {
  if (!Array.isArray(value)) throw new Error(`${where}: expected an array`);
  return value;
}
/** @param {unknown} value @param {string} where */
export function strings(value, where) {
  return array(value, where).map((v) => string(v, where));
}
/** @param {unknown} value @param {string} where */
function optionalString(value, where) {
  return value === undefined ? undefined : string(value, where);
}
/** @param {unknown} value @param {string} where */
function optionalStrings(value, where) {
  return value === undefined ? undefined : strings(value, where);
}
/** @param {unknown} value @param {string} where */
function condition(value, where) {
  return typeof value === "boolean" || value === undefined ? value : string(value, where);
}
/** @param {unknown} value @param {string} where */
function optionalObject(value, where) {
  return value === undefined ? undefined : object(value, where);
}
/** @param {unknown} value @returns {import('./models.js').Result} */
function result(value) {
  if (value === "success" || value === "failure" || value === "cancelled" || value === "skipped")
    return value;
  throw new Error("invalid job outcome");
}
/** @param {unknown} value @param {string} where */
function boolean(value, where) {
  if (typeof value !== "boolean") throw new Error(`${where}: expected boolean`);
  return value;
}
/** @param {unknown} value @param {string} where */
function stringMap(value, where) {
  return Object.fromEntries(
    Object.entries(object(value, where)).map(([key, v]) => [key, string(v, where)]),
  );
}
// Fields the resolver models, or that cannot change which runner a job requests
// or how its outcome is reported. Anything else is refused rather than dropped.
const jobFields = new Set([
  "name",
  "uses",
  "if",
  "needs",
  "runs-on",
  "strategy",
  "steps",
  "with",
  "secrets",
  "outputs",
  "permissions",
  "environment",
  "env",
  "defaults",
  "container",
  "concurrency",
  "timeout-minutes",
]);
const stepFields = new Set(["name", "id", "uses", "run", "with", "env", "if", "shell"]);
const strategyFields = new Set(["matrix", "fail-fast", "max-parallel"]);
/** @param {Record<string,unknown>} raw @param {Set<string>} allowed @param {string} where */
function onlyFields(raw, allowed, where) {
  for (const key of Object.keys(raw))
    if (!allowed.has(key)) throw new Error(`${where}: unsupported field ${key}`);
}
/** The workload every telemetry recording must cover: event classes × diff classes × PR ownership. */
export const workload = {
  eventClasses: [
    "pr-opened",
    "pr-synchronize",
    "pr-reopened",
    "base-push",
    "release-pr-merge",
    "release-tag",
    "release-published",
    "downstream",
    "dispatch",
    "schedule",
    "manual-default",
    "manual-opt-in",
  ],
  diffs: ["source", "version-only", "dependency"],
  ownerships: ["same-repository", "fork"],
  optionalPublish: ["crates", "pypi", "npm"],
};
/** @param {unknown} value @returns {import('./models.js').Workflow} */
export function workflow(value) {
  const doc = object(value, "workflow");
  const on =
    typeof doc.on === "string"
      ? { [doc.on]: null }
      : Array.isArray(doc.on)
        ? Object.fromEntries(strings(doc.on, "on").map((n) => [n, null]))
        : object(doc.on, "on");
  /** @type {Record<string,import('./models.js').Trigger|null>} */
  const triggers = {};
  for (const [event, raw] of Object.entries(on)) {
    if (raw === null) {
      triggers[event] = null;
      continue;
    }
    if (event === "schedule") {
      array(raw, "schedule");
      triggers[event] = {};
      continue;
    }
    const filter = object(raw, `on.${event}`);
    const inputs =
      filter.inputs === undefined
        ? undefined
        : Object.fromEntries(
            Object.entries(object(filter.inputs, "inputs")).map(([key, spec]) => [
              key,
              object(spec, "input specification"),
            ]),
          );
    triggers[event] = {
      ...filter,
      branches: optionalStrings(filter.branches, "branches"),
      tags: optionalStrings(filter.tags, "tags"),
      types: optionalStrings(filter.types, "types"),
      paths: optionalStrings(filter.paths, "paths"),
      workflows: optionalStrings(filter.workflows, "workflows"),
      inputs,
    };
  }
  /** @type {Record<string,import('./models.js').Job>} */
  const jobs = {};
  for (const [id, raw] of Object.entries(object(doc.jobs, "workflow.jobs"))) {
    const job = object(raw, `jobs.${id}`);
    if ("continue-on-error" in job) throw new Error("unresolved continue-on-error outcome");
    onlyFields(job, jobFields, `jobs.${id}`);
    const needs = typeof job.needs === "string" ? job.needs : optionalStrings(job.needs, "needs");
    const runner =
      typeof job["runs-on"] === "string"
        ? job["runs-on"]
        : optionalStrings(job["runs-on"], "runner");
    const rawStrategy = optionalObject(job.strategy, "strategy");
    if (rawStrategy) onlyFields(rawStrategy, strategyFields, `jobs.${id}.strategy`);
    const strategy = rawStrategy
      ? { ...rawStrategy, matrix: optionalObject(rawStrategy.matrix, "matrix") }
      : undefined;
    const steps =
      job.steps === undefined
        ? undefined
        : array(job.steps, "steps").map((rawStep) => {
            const step = object(rawStep, "step");
            if ("continue-on-error" in step || "timeout-minutes" in step)
              throw new Error("unresolved step outcome control");
            onlyFields(step, stepFields, `jobs.${id} step`);
            return {
              run: optionalString(step.run, "step.run"),
              uses: optionalString(step.uses, "step.uses"),
              if: condition(step.if, "step.if"),
              with: optionalObject(step.with, "step.with"),
              env: optionalObject(step.env, "step.env"),
            };
          });
    jobs[id] = {
      name: optionalString(job.name, "job.name"),
      uses: optionalString(job.uses, "job.uses"),
      if: condition(job.if, "job.if"),
      needs,
      "runs-on": runner,
      strategy,
      steps,
      with: optionalObject(job.with, "job.with"),
      outputs: job.outputs === undefined ? undefined : stringMap(job.outputs, "outputs"),
    };
  }
  // Returning constructed records lets strict tsc check the decoder against the
  // model definitions; no unchecked deserialization assertion bridges them.
  return { on: triggers, jobs };
}
/** @param {unknown} value @returns {import('./models.js').Telemetry} */
export function telemetry(value) {
  const doc = object(value, "telemetry");
  if (doc.schema_version !== 1) throw new Error("unsupported telemetry schema");
  const runs = array(doc.runs, "runs").map((raw) => {
    const run = object(raw, "run");
    const jobs = array(run.jobs, "jobs").map((rawJob) => {
      const job = object(rawJob, "job");
      return {
        workflow: string(job.workflow, "workflow"),
        event: string(job.event, "event"),
        job: string(job.job, "job"),
        name: string(job.name, "name"),
        runner: string(job.runner, "runner"),
        target: job.target,
        requested: boolean(job.requested, "requested"),
        result: result(job.result),
      };
    });
    return {
      id: string(run.id, "run.id"),
      scenario: string(run.scenario, "scenario"),
      event: string(run.event, "event"),
      workflow: string(run.workflow, "workflow"),
      manual: boolean(run.manual, "manual"),
      jobs,
    };
  });
  const ids = new Set(runs.map((run) => run.id));
  if (!runs.length || ids.size !== runs.length) throw new Error("empty or duplicate run telemetry");
  const scenarios = array(doc.scenarios, "scenarios").map((raw) => {
    const scenario = object(raw, "scenario");
    const classification = object(scenario.classification, "scenario classification");
    return {
      label: string(scenario.label, "scenario.label"),
      classification: {
        isReleaseOnly: boolean(classification.isReleaseOnly, "isReleaseOnly"),
        macosRequired: boolean(classification.macosRequired, "macosRequired"),
        reason: string(classification.reason, "reason"),
      },
    };
  });
  const labels = new Set(scenarios.map((scenario) => scenario.label));
  if (labels.size !== scenarios.length) throw new Error("duplicate scenario telemetry");
  for (const eventClass of workload.eventClasses)
    for (const diff of workload.diffs)
      for (const ownership of workload.ownerships)
        if (!labels.has(`${eventClass}/${diff}/${ownership}`))
          throw new Error(`incomplete workload: missing ${eventClass}/${diff}/${ownership}`);
  for (const run of runs)
    if (!labels.has(run.scenario)) throw new Error(`run ${run.id} names no recorded scenario`);
  const lifecycles = array(doc.lifecycles, "lifecycles").map((raw) => {
    const lifecycle = object(raw, "lifecycle");
    const references = strings(lifecycle.runs, "lifecycle.runs");
    if (!references.length) throw new Error("lifecycle references no runs");
    if (new Set(references).size !== references.length)
      throw new Error("duplicate lifecycle run references");
    for (const id of references) {
      if (!ids.has(id)) throw new Error(`unresolved lifecycle run ${id}`);
      if (runs.some((r) => r.id === id && r.manual))
        throw new Error("manual diagnostic in automatic lifecycle");
    }
    const optionalPublish = Object.fromEntries(
      Object.entries(object(lifecycle.optionalPublish, "optionalPublish")).map(([k, v]) => [
        k,
        boolean(v, "publication option"),
      ]),
    );
    return { id: string(lifecycle.id, "lifecycle.id"), runs: references, optionalPublish };
  });
  if (!lifecycles.length) throw new Error("empty lifecycle telemetry");
  for (const diff of workload.diffs)
    for (const ownership of workload.ownerships)
      for (let combination = 0; combination < 2 ** workload.optionalPublish.length; combination++) {
        const options = Object.fromEntries(
          workload.optionalPublish.map((name, bit) => [name, Boolean(combination & (1 << bit))]),
        );
        if (
          !lifecycles.some(
            (l) =>
              l.id.startsWith(`${diff}/${ownership}/`) &&
              workload.optionalPublish.every((name) => l.optionalPublish[name] === options[name]),
          )
        )
          throw new Error(
            `incomplete workload: no ${diff}/${ownership} lifecycle for ${JSON.stringify(options)}`,
          );
      }
  return {
    schema_version: 1,
    sourceDigest: string(doc.sourceDigest, "sourceDigest"),
    scenarios,
    runs,
    lifecycles,
  };
}
/** @param {unknown} value @returns {import('./models.js').Observation} */
export function observation(value) {
  const doc = object(value, "observation");
  const runs = array(doc.runs, "observed runs").map((raw) => {
    const run = object(raw, "observed run");
    const jobs = array(run.jobs, "observed jobs").map((rawJob) => {
      const job = object(rawJob, "observed job");
      return {
        name: string(job.name, "name"),
        status: string(job.status, "status"),
        conclusion: job.conclusion === null ? null : string(job.conclusion, "conclusion"),
      };
    });
    return {
      name: string(run.name, "name"),
      html_url: string(run.html_url, "html_url"),
      status: string(run.status, "status"),
      conclusion: run.conclusion === null ? null : string(run.conclusion, "conclusion"),
      jobs,
    };
  });
  return { head: string(doc.head, "observation.head"), runs };
}
