import { execFileSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
// Private interpreter for this repository's workflow contract; unknown routing is fatal.
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { Evaluator, Lexer, Parser, data } from "@actions/expressions";
import { truthy } from "@actions/expressions/result";
import YAML from "yaml";
import { array, object, string, workflow } from "./validation.mjs";

/** @param {unknown} v @returns {import('@actions/expressions/data/expressiondata').ExpressionData} */
function value(v) {
  if (v == null) return new data.Null();
  if (typeof v === "boolean") return new data.BooleanData(v);
  if (typeof v === "number") return new data.NumberData(v);
  if (typeof v === "string") return new data.StringData(v);
  if (Array.isArray(v)) return new data.Array(...v.map(value));
  return new data.Dictionary(
    ...Object.entries(object(v, "expression context")).map(([key, v]) => ({
      key,
      value: value(v),
    })),
  );
}
/** @param {string|boolean|undefined|null} expression @param {Record<string,unknown>} context @param {string[]} outcomes */
export function evaluate(expression, context, outcomes = []) {
  if (expression === undefined || expression === null)
    return outcomes.every((o) => o === "success");
  if (typeof expression === "boolean") return expression;
  const src = expression.replace(/^\s*\$\{\{([\s\S]*)\}\}\s*$/, "$1");
  /** @type {Map<string,import('@actions/expressions/funcs/info').FunctionDefinition>} */
  const funcs = new Map(
    ["always", "success", "failure", "cancelled"].map((name) => [
      name,
      {
        name,
        minArgs: 0,
        maxArgs: 0,
        call: () =>
          new data.BooleanData(
            name === "always" ||
              (name === "success"
                ? outcomes.every((o) => o === "success")
                : outcomes.includes(name === "failure" ? "failure" : "cancelled")),
          ),
      },
    ]),
  );
  const ast = new Parser(new Lexer(src).lex().tokens, Object.keys(context), [
    ...funcs.values(),
  ]).parse();
  const dataContext = value(context);
  if (!(dataContext instanceof data.Dictionary)) throw new Error("context must be a dictionary");
  return new Evaluator(ast, dataContext, funcs).evaluate();
}
/** @param {boolean|import('@actions/expressions/data/expressiondata').ExpressionData} v */
function truth(v) {
  return typeof v === "boolean" ? v : truthy(v);
}
/** @param {string} src @param {Record<string,unknown>} ctx */
function interpolate(src, ctx) {
  return src.replace(/\$\{\{([\s\S]*?)\}\}/g, (_, expression) => {
    const evaluated = evaluate(expression, ctx);
    return typeof evaluated === "boolean" ? String(evaluated) : evaluated.coerceString();
  });
}
/** Evaluate a step's declared env exactly as the runner would hand it to `run`. @param {{env?: Record<string, unknown>}} step @param {Record<string, unknown>} ctx @returns {Record<string, string>} */
export function stepEnv(step, ctx) {
  return Object.fromEntries(
    Object.entries(step.env ?? {}).map(([key, raw]) => [
      key,
      typeof raw === "string" ? interpolate(raw, ctx) : String(raw),
    ]),
  );
}
export function loadGraph(directory = ".github/workflows") {
  /** @type {import('./models.js').Graph} */
  const graph = {};
  for (const name of readdirSync(directory)
    .filter((n) => n.endsWith(".yml"))
    .sort()) {
    const doc = workflow(
      YAML.parse(readFileSync(`${directory}/${name}`, "utf8"), { uniqueKeys: true }),
    );
    if (!doc || !doc.on || !doc.jobs) throw new Error(`${name}: invalid workflow`);
    graph[name] = doc;
  }
  const external = object(
    JSON.parse(readFileSync(".github/reusable-workflows.json", "utf8")),
    "reusable workflow lock",
  );
  for (const [call, rawLock] of Object.entries(external)) {
    const lock = object(rawLock, "reusable lock entry");
    const path = string(lock.path, "lock.path");
    const digest = string(lock.sha256, "lock.sha256");
    string(lock.commit, "lock.commit");
    let src = readFileSync(path, "utf8");
    for (const rawAnnotation of array(lock.annotations ?? [], "local annotations")) {
      const annotation = object(rawAnnotation, "local annotation");
      const before = string(annotation.before, "annotation site");
      const reason = string(annotation.reason, "annotation rationale");
      const indent = before.slice(0, before.length - before.trimStart().length);
      const marker = `${indent}# llmlint: ignore[comments_earn_their_place] ${reason}`;
      const annotated = `${marker}\n${before}`;
      if (src.split(annotated).length !== 2)
        throw new Error("upstream annotation moved, duplicated or missing");
      src = src.replace(annotated, before);
    }
    if (createHash("sha256").update(src).digest("hex") !== digest)
      throw new Error(`reusable workflow digest mismatch: ${call}`);
    graph[call] = workflow(YAML.parse(src.toString()));
  }
  return graph;
}
export function sourceDigest() {
  const files = execFileSync(
    "git",
    [
      "ls-files",
      "--cached",
      "--others",
      "--exclude-standard",
      "-z",
      ".github",
      "scripts/stage-release-product.sh",
      "scripts/check-release-identity.sh",
      "scripts/stage-and-verify-release-product.py",
      "scripts/verify-release-archive.sh",
      "scripts/smoke-python-bundle.sh",
      "scripts/smoke-npm-bundle.sh",
      "release-platforms.toml",
      "release-targets.toml",
      "screencomp.toml",
      "pnpm-lock.yaml",
      "pnpm-workspace.yaml",
    ],
    { encoding: "utf8" },
  )
    .split("\0")
    .filter((file) => Boolean(file) && file !== ".github/budgets.yaml")
    .sort();
  const hash = createHash("sha256");
  for (const file of files) hash.update(file).update(readFileSync(file));
  return hash.digest("hex");
}
/** @param {string} pattern @param {string} text */
function glob(pattern, text) {
  return new RegExp(
    `^${pattern
      .replace(/[.+^${}()|[\]\\]/g, "\\$&")
      .replaceAll("**", "\u0000")
      .replaceAll("*", "[^/]*")
      .replaceAll("\u0000", ".*")
      .replaceAll("?", ".")}$`,
  ).test(text);
}
/** @param {import('./models.js').Workflow} workflow @param {import('./models.js').Scenario} scenario */
export function triggered(workflow, scenario) {
  const on = workflow.on;
  const names =
    scenario.event_name === "pull_request"
      ? ["pull_request", "pull_request_target"]
      : [scenario.event_name];
  return names.some((name) => {
    if (!(name in on)) return false;
    const filter = on[name] ?? {};
    for (const key of Object.keys(filter))
      if (
        ![
          "branches",
          "tags",
          "types",
          "paths",
          "workflows",
          "inputs",
          "secrets",
          "outputs",
        ].includes(key) &&
        name !== "schedule"
      )
        throw new Error(`unsupported event filter ${key}`);
    if (filter.types && !filter.types.includes(scenario.action ?? "completed")) return false;
    if (name === "push") {
      const tag = scenario.ref.startsWith("refs/tags/");
      if (filter.tags || filter.branches) {
        const filters = tag ? filter.tags : filter.branches;
        if (!filters?.some((p) => glob(p, scenario.ref.replace(/^refs\/(heads|tags)\//, ""))))
          return false;
      }
    }
    if (
      filter.branches &&
      name !== "push" &&
      !filter.branches.some((p) =>
        glob(p, scenario.pull_request?.base?.ref ?? scenario.workflow_run?.head_branch ?? ""),
      )
    )
      return false;
    if (filter.paths && !scenario.changed.some((f) => (filter.paths ?? []).some((p) => glob(p, f))))
      return false;
    if (filter.workflows && !filter.workflows.includes(scenario.workflow_run?.name ?? ""))
      return false;
    return true;
  });
}
/** @param {Record<string,unknown>} matrix */
export function matrixRows(matrix = {}) {
  if (typeof matrix !== "object" || Array.isArray(matrix))
    throw new Error("unresolved matrix expression");
  /** @type {Record<string,unknown>[]} */
  let rows = [{}];
  for (const [key, axis] of Object.entries(matrix)) {
    if (["include", "exclude"].includes(key)) continue;
    if (!Array.isArray(axis)) throw new Error(`unresolved matrix axis ${key}`);
    rows = rows.flatMap((row) => axis.map((v) => ({ ...row, [key]: v })));
  }
  const original = Object.keys(matrix).some((k) => !["include", "exclude"].includes(k))
    ? rows.map((r) => ({ ...r }))
    : [];
  rows = original.length ? rows : [];
  for (const rawInc of array(matrix.include ?? [], "matrix.include")) {
    const inc = object(rawInc, "matrix.include row");
    let merged = false;
    original.forEach((row, i) => {
      if (Object.keys(row).every((k) => !(k in inc) || row[k] === inc[k])) {
        rows[i] = { ...rows[i], ...inc };
        merged = true;
      }
    });
    if (!merged) rows.push({ ...inc });
  }
  rows = rows.filter(
    (row) =>
      !array(matrix.exclude ?? [], "matrix.exclude").some((ex) =>
        Object.entries(object(ex, "matrix.exclude row")).every(([k, v]) => row[k] === v),
      ),
  );
  return Object.keys(matrix).length ? rows : [{}];
}
/** @param {string|string[]|undefined} needs @returns {string[]} */
function needsList(needs) {
  return typeof needs === "string" ? [needs] : (needs ?? []);
}
/** @param {import('./models.js').Graph} graph @param {string} file @param {import('./models.js').Scenario} scenario @param {import('./models.js').Classification} classification @param {Record<string,import('./models.js').Result>} overrides @param {string[]} stack @param {Record<string,unknown>} inputs @returns {import('./models.js').ResolvedRun} */
export function resolveRun(
  graph,
  file,
  scenario,
  classification,
  overrides = {},
  stack = [],
  inputs = {},
) {
  if (stack.includes(file))
    throw new Error(`workflow call cycle: ${[...stack, file].join(" -> ")}`);
  const wf = graph[file];
  if (!wf) throw new Error(`unresolved workflow call ${file}`);
  const pending = new Set(Object.keys(wf.jobs));
  /** @type {Record<string,import('./models.js').Outcome>} */
  const results = {};
  /** @type {import('./models.js').Row[]} */
  const rows = [];
  while (pending.size) {
    const id = [...pending].find((id) => needsList(wf.jobs[id].needs).every((n) => n in results));
    if (!id) throw new Error(`${file}: unresolved needs or cycle`);
    pending.delete(id);
    const job = wf.jobs[id];
    const needs = Object.fromEntries(needsList(job.needs).map((n) => [n, results[n]]));
    const outcomes = Object.values(needs).map((n) => n.result);
    const github = {
      event_name: scenario.event_name,
      ref: scenario.ref,
      ref_name: scenario.ref?.replace(/^refs\/(heads|tags)\//, ""),
      repository: "nickderobertis/skilltest",
      event: scenario,
      actor: scenario.sender?.login ?? "nickderobertis",
    };
    /** @type {Record<string,unknown>} */
    const ctx = {
      github,
      needs,
      inputs: { ...inputs, ...scenario.inputs },
      matrix: {},
      secrets: {},
      env: {},
      vars: {},
    };
    let runs = truth(evaluate(job.if, ctx, outcomes));
    if (!/\b(always|failure|success|cancelled)\s*\(/.test(String(job.if ?? "")))
      runs &&= outcomes.every((o) => o === "success");
    /** @type {import('./models.js').Result} */
    let result = runs ? "success" : "skipped";
    if (runs && file === "ci.yml" && id === "mac-result") {
      const step = (job.steps ?? []).find((candidate) => candidate.run);
      if (step?.run !== "node .github/required-result.mjs")
        throw new Error("unresolved required result command");
      // The carrier reads only what its workflow step wires into env.
      const outcome = spawnSync("bash", ["-c", step.run], {
        env: { ...process.env, ...stepEnv(step, ctx) },
        encoding: "utf8",
      });
      result = outcome.status === 0 ? "success" : "failure";
    }
    const resolvedMatrix = Object.fromEntries(
      Object.entries(job.strategy?.matrix ?? {}).map(([key, axis]) => {
        if (typeof axis !== "string") return [key, axis];
        if (!runs) return [key, []];
        const evaluated = evaluate(axis, ctx, outcomes);
        if (!(evaluated instanceof data.Array))
          throw new Error(`${file}:${id}: unresolved matrix ${key}`);
        return [key, evaluated.values().map((v) => v.coerceString())];
      }),
    );
    const expanded = matrixRows(resolvedMatrix);
    for (const matrix of expanded) {
      ctx.matrix = matrix;
      const name = interpolate(
        job.name ??
          id + (Object.keys(matrix).length ? ` (${Object.values(matrix).join(", ")})` : ""),
        ctx,
      );
      if (job.uses) {
        const callee = job.uses.startsWith("./.github/workflows/")
          ? string(job.uses.split("/").at(-1), "callee")
          : job.uses;
        if (!graph[callee])
          throw new Error(`${file}:${id}: unresolved reusable workflow ${job.uses}`);
        if (!graph[callee]?.on?.workflow_call) {
          // YAML null is a valid declaration.
          if (!Object.hasOwn(graph[callee]?.on ?? {}, "workflow_call"))
            throw new Error(`unresolved workflow_call ${callee}`);
        }
        if (runs) {
          const callInputs = Object.fromEntries(
            Object.entries(graph[callee].on.workflow_call?.inputs ?? {}).map(([key, spec]) => [
              key,
              spec.default,
            ]),
          );
          Object.assign(callInputs, job.with ?? {});
          const sub = resolveRun(
            graph,
            callee,
            scenario,
            classification,
            overrides,
            [...stack, file],
            callInputs,
          );
          rows.push(...sub.rows.map((row) => ({ ...row, caller: `${file}:${id}` })));
          if (Object.values(sub.results).some((r) => r.result === "failure")) result = "failure";
        }
        // A skipped call requests no runner; its row keeps the job accounted for.
        else
          rows.push({
            workflow: file,
            event: scenario.label ?? scenario.event_name,
            job: id,
            name,
            runner: "none (skipped call)",
            target: null,
            requested: false,
            result: "skipped",
          });
      } else {
        if (typeof job["runs-on"] !== "string" && !Array.isArray(job["runs-on"]))
          throw new Error(`${file}:${id}: unresolved runner`);
        const runner = interpolate(needsList(job["runs-on"]).join(","), ctx);
        if (!runner || runner.includes("${{") || !/^(ubuntu|macos|windows)-[\w.,-]+$/.test(runner))
          throw new Error(`${file}:${id}: unresolved runner ${runner}`);
        /** @type {import('./models.js').Result} */
        const outcome = overrides[`${file}:${id}`] ?? result;
        rows.push({
          workflow: file,
          event: scenario.label ?? scenario.event_name,
          job: id,
          name,
          runner,
          target: matrix.target ?? (id === "darwin" ? "aarch64-apple-darwin" : null),
          requested: runs,
          result: outcome,
        });
        if (runs && outcome !== "success") result = outcome;
      }
    }
    /** @type {Record<string,string>} */
    let outputs = {};
    if (id === "policy" && result === "success") {
      if (
        job.outputs?.macosRequired !== "${{ steps.classify.outputs.macosRequired }}" ||
        !(job.steps ?? []).some((step) =>
          step.run?.startsWith("node .github/macos-policy.mjs classify --event "),
        )
      )
        throw new Error("unresolved production policy output mapping");
      outputs = { macosRequired: String(classification.macosRequired) };
    }
    if (file.startsWith("nickderobertis/screencomp/") && id === "arches" && result === "success") {
      if (
        job.outputs?.matrix !== "${{ steps.read.outputs.matrix }}" ||
        !(job.steps ?? []).some((step) => step.run?.includes("screencomp arches --format json"))
      )
        throw new Error("unresolved screencomp matrix producer");
      const arches = JSON.parse(
        execFileSync(
          process.env.SCREENCOMP_BIN ??
            (existsSync(".github/.tools/screencomp") ? ".github/.tools/screencomp" : "screencomp"),
          ["arches", "--format", "json"],
          { encoding: "utf8" },
        ),
      );
      const actualArches = array(arches, "screencomp arches output").map((v) =>
        string(v, "screencomp arch"),
      );
      if (!actualArches.length) throw new Error("empty screencomp architecture matrix");
      outputs = {
        matrix: inputs.arches
          ? string(inputs.arches, "arches input")
          : JSON.stringify(actualArches),
      };
    }
    results[id] = { result, outputs };
  }
  return { rows, results };
}
/** @param {import('./models.js').Graph} graph @param {import('./models.js').Scenario} scenario @param {import('./models.js').Classification} classification */
export function scenarioRuns(graph, scenario, classification) {
  const runs = [];
  for (const [file, wf] of Object.entries(graph))
    if (triggered(wf, scenario))
      runs.push({ workflow: file, ...resolveRun(graph, file, scenario, classification) });
  return runs;
}
