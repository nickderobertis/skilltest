/** @param {unknown} value @param {string} where @returns {Record<string,unknown>} */
export function object(value, where) {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw new Error(`${where}: expected an object`);
  return /** @type {Record<string,unknown>} */ (value);
}
/** @param {unknown} value @param {string} where @returns {string} */
export function string(value, where) {
  if (typeof value !== "string" || !value) throw new Error(`${where}: expected a nonempty string`);
  return value;
}
/** @param {unknown} value @param {string} where @returns {unknown[]} */
export function array(value, where) {
  if (!Array.isArray(value)) throw new Error(`${where}: expected an array`);
  return value;
}
/** @param {unknown} value @param {string} where @returns {string[]} */
export function strings(value, where) {
  return array(value, where).map((v) => string(v, where));
}
/** @param {unknown} value @returns {import('./models.js').Workflow} */
export function workflow(value) {
  const doc = object(value, "workflow");
  if (typeof doc.on === "string") doc.on = { [doc.on]: null };
  if (Array.isArray(doc.on))
    doc.on = Object.fromEntries(strings(doc.on, "on").map((name) => [name, null]));
  const triggers = object(doc.on, "workflow.on");
  for (const [event, raw] of Object.entries(triggers)) {
    if (raw === null) continue;
    if (event === "schedule") {
      array(raw, "schedule");
      triggers[event] = {};
      continue;
    }
    const filter = object(raw, `on.${event}`);
    for (const key of ["types", "branches", "tags", "paths", "workflows"])
      if (key in filter) strings(filter[key], `on.${event}.${key}`);
    if (filter.inputs)
      for (const spec of Object.values(object(filter.inputs, "inputs")))
        object(spec, "input specification");
  }
  const jobs = object(doc.jobs, "workflow.jobs");
  for (const [id, raw] of Object.entries(jobs)) {
    const job = object(raw, `jobs.${id}`);
    for (const key of ["name", "uses"]) if (key in job) string(job[key], `${id}.${key}`);
    if (job.if !== undefined && typeof job.if !== "boolean") string(job.if, `${id}.if`);
    if (job.needs !== undefined)
      typeof job.needs === "string" ? string(job.needs, "needs") : strings(job.needs, "needs");
    if (job["runs-on"] !== undefined)
      typeof job["runs-on"] === "string"
        ? string(job["runs-on"], "runner")
        : strings(job["runs-on"], "runner");
    if (job.strategy !== undefined) {
      const strategy = object(job.strategy, "strategy");
      if (strategy.matrix !== undefined) object(strategy.matrix, "matrix");
    }
    if (job.with !== undefined) object(job.with, "with");
    if (job.outputs !== undefined)
      for (const output of Object.values(object(job.outputs, "outputs")))
        string(output, "output expression");
    if (job.steps !== undefined)
      for (const rawStep of array(job.steps, "steps")) {
        const step = object(rawStep, "step");
        for (const key of ["run", "uses"]) if (key in step) string(step[key], `step.${key}`);
        for (const key of ["with", "env"]) if (key in step) object(step[key], `step.${key}`);
      }
  }
  return /** @type {import('./models.js').Workflow} */ (/** @type {unknown} */ (doc));
}
/** @param {unknown} value @returns {import('./models.js').Telemetry} */
export function telemetry(value) {
  const doc = object(value, "telemetry");
  if (doc.schema_version !== 1) throw new Error("unsupported telemetry schema");
  string(doc.sourceDigest, "sourceDigest");
  array(doc.scenarios, "scenarios");
  const runs = array(doc.runs, "runs");
  const ids = new Set();
  if (!runs.length) throw new Error("empty run telemetry");
  for (const raw of runs) {
    const run = object(raw, "run");
    const id = string(run.id, "run.id");
    if (ids.has(id)) throw new Error(`duplicate run ${id}`);
    ids.add(id);
    for (const key of ["scenario", "event", "workflow"]) string(run[key], `run.${key}`);
    if (typeof run.manual !== "boolean") throw new Error("run.manual must be boolean");
    for (const rawJob of array(run.jobs, "run.jobs")) {
      const job = object(rawJob, "job");
      for (const key of ["workflow", "event", "job", "name", "runner"])
        string(job[key], `job.${key}`);
      if (
        typeof job.requested !== "boolean" ||
        !["success", "failure", "cancelled", "skipped"].includes(String(job.result))
      )
        throw new Error("invalid job outcome");
    }
  }
  const lifecycles = array(doc.lifecycles, "lifecycles");
  if (!lifecycles.length) throw new Error("empty lifecycle telemetry");
  for (const raw of lifecycles) {
    const lifecycle = object(raw, "lifecycle");
    string(lifecycle.id, "lifecycle.id");
    for (const id of strings(lifecycle.runs, "lifecycle.runs")) {
      if (!ids.has(id)) throw new Error(`unresolved lifecycle run ${id}`);
      if (runs.some((raw) => object(raw, "run").id === id && object(raw, "run").manual === true))
        throw new Error("manual diagnostic in automatic lifecycle");
    }
    for (const choice of Object.values(object(lifecycle.optionalPublish, "optionalPublish")))
      if (typeof choice !== "boolean") throw new Error("invalid publication option");
  }
  return /** @type {import('./models.js').Telemetry} */ (/** @type {unknown} */ (doc));
}
/** @param {unknown} value @returns {import('./models.js').Observation} */
export function observation(value) {
  const doc = object(value, "observation");
  string(doc.head, "observation.head");
  for (const raw of array(doc.runs, "observed runs")) {
    const run = object(raw, "observed run");
    for (const key of ["name", "html_url", "status"]) string(run[key], key);
    if (run.conclusion !== null) string(run.conclusion, "conclusion");
    for (const rawJob of array(run.jobs, "observed jobs")) {
      const job = object(rawJob, "observed job");
      string(job.name, "name");
      string(job.status, "status");
      if (job.conclusion !== null) string(job.conclusion, "conclusion");
    }
  }
  return /** @type {import('./models.js').Observation} */ (/** @type {unknown} */ (doc));
}
