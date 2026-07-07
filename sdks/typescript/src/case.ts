/**
 * Define a full test case in code — everything a `*.skilltest.yaml` file
 * carries — and hand it straight to {@link import("./runner.js").runSkill | runSkill}.
 *
 * ```ts
 * import { runSkill, testCase, boolean, numeric, user } from "@skill-test/sdk";
 *
 * const report = await runSkill(
 *   testCase({
 *     skill: "skills/greeter",
 *     input: "Greet Dr. Smith, who has an appointment today.",
 *     evals: [
 *       boolean("the reply greets Dr. Smith by name"),
 *       numeric("how warm is the tone", { min: 0, max: 10, threshold: 7 }),
 *     ],
 *   }),
 * );
 * if (!report.passed) throw new Error("evals failed");
 * ```
 *
 * A {@link TestCaseInput} is the code twin of the YAML schema (`docs/schema.md`):
 * `skill` resolves relative to the working directory, `input` is the first user
 * message, `user` turns the case multi-turn, `mocks` are the same
 * {@link import("./mock.js").spy | spy} / {@link import("./mock.js").stub | stub}
 * / {@link import("./mock.js").deny | deny} / {@link import("./mock.js").rewrite | rewrite}
 * objects you would pass to `runSkill({ mocks })` (bound for assertions after
 * the run, and referenceable by name from a `called`/`notCalled` eval), and
 * `evals` decide pass/fail. The CLI validates the compiled case, so a malformed
 * one throws a {@link import("./errors.js").SkilltestUsageError | SkilltestUsageError}.
 *
 * Writing cases in code is the recommended approach; YAML files remain fully
 * supported (`runSkill("cases/greet.yaml")`) and are what `discover` collects.
 */
import { SkilltestUsageError } from "./errors.js";
import { type Criterion, ToolMock, type ToolSpy, type WhereCriteria } from "./mock.js";

/** One eval, built with {@link boolean} / {@link numeric} / {@link called} /
 * {@link notCalled}. Opaque: it carries the JSON the CLI validates. */
export interface Eval {
  readonly json: Record<string, unknown>;
}

/** The simulated-user block that makes a case multi-turn (see {@link user}). */
export interface SimulatedUser {
  persona: string;
  doneWhen?: string;
  maxTurns?: number;
}

/** A full test case defined in code — the twin of a `*.skilltest.yaml` file.
 * Pass one (via {@link testCase}) to {@link import("./runner.js").runSkill}. */
export interface TestCaseInput {
  /** Skill directory under test (a dir containing SKILL.md), resolved relative
   * to the working directory. */
  skill: string;
  /** The initial data/prompt handed to the skill as the first user message. */
  input: string;
  /** The evals that decide pass/fail (must be non-empty). */
  evals: Eval[];
  /** Optional report label (defaults to `case`). */
  name?: string;
  /** Present ⇒ multi-turn (see {@link user}). */
  user?: SimulatedUser;
  /** Mock/spy objects for this case — the same builders `runSkill({ mocks })`
   * takes. Bound for assertions after the run; a named one is referenceable
   * from a `called`/`notCalled` eval. */
  mocks?: ToolSpy[];
  /** Force the observation channel even without mocks (implied when `mocks` is
   * non-empty), so the report carries `mock_calls`. */
  spy?: boolean;
}

/** Assert a plain-English `criterion` holds (or, with `expected: false`, that
 * it does not). Scored by the judge against the transcript. */
export function boolean(
  criterion: string,
  options: { expected?: boolean; name?: string } = {},
): Eval {
  const json: Record<string, unknown> = { type: "boolean", criterion };
  json.expected = options.expected ?? true;
  if (options.name !== undefined) json.name = options.name;
  return { json };
}

/** Score `criterion` on the `[min, max]` scale and pass when the score
 * satisfies `comparator` (`>=` `>` `<=` `<`, default `>=`) against `threshold`. */
export function numeric(
  criterion: string,
  options: { min: number; max: number; threshold: number; comparator?: string; name?: string },
): Eval {
  const json: Record<string, unknown> = {
    type: "numeric",
    criterion,
    min: options.min,
    max: options.max,
    threshold: options.threshold,
    comparator: options.comparator ?? ">=",
  };
  if (options.name !== undefined) json.name = options.name;
  return { json };
}

/** Deterministic (no judge): assert the named `mock`/spy observed at least one
 * matching call, or exactly `times`. Reference a spy/stub/… by the `name` you
 * gave it in the case's `mocks`. `where` narrows by input field. */
export function called(
  mock: string,
  options: { times?: number; where?: WhereCriteria; name?: string } = {},
): Eval {
  const json: Record<string, unknown> = { type: "called", mock };
  if (options.times !== undefined) json.times = options.times;
  if (options.where !== undefined) json.where = compileWhere(options.where);
  if (options.name !== undefined) json.name = options.name;
  return { json };
}

/** Deterministic: assert the named `mock`/spy observed **no** matching call
 * (optionally narrowed by `where`). */
export function notCalled(
  mock: string,
  options: { where?: WhereCriteria; name?: string } = {},
): Eval {
  const json: Record<string, unknown> = { type: "not_called", mock };
  if (options.where !== undefined) json.where = compileWhere(options.where);
  if (options.name !== undefined) json.name = options.name;
  return { json };
}

/** A simulated user for a multi-turn case: after each assistant turn the judge
 * plays `persona` until `doneWhen` holds, the skill reports done, or `maxTurns`
 * is reached. */
export function user(
  persona: string,
  options: { doneWhen?: string; maxTurns?: number } = {},
): SimulatedUser {
  return { persona, doneWhen: options.doneWhen, maxTurns: options.maxTurns };
}

/**
 * Build a code-defined case. A convenience identity over {@link TestCaseInput}
 * so `runSkill(testCase({...}))` reads well and infers the eval types; you may
 * also pass a plain object literal to `runSkill` directly.
 */
export function testCase(input: TestCaseInput): TestCaseInput {
  return input;
}

/** @internal Whether a value is a code-defined case (vs a path string). */
export function isTestCaseInput(value: unknown): value is TestCaseInput {
  return typeof value === "object" && value !== null && "skill" in value && "evals" in value;
}

/** @internal The JSON the CLI ingests via `--case-json`: assigns names to the
 * case's mocks (so binding and `called`/`notCalled` references resolve) and
 * turns the spy channel on when any mock/spy is present. */
export function compileCase(input: TestCaseInput): Record<string, unknown> {
  const payload: Record<string, unknown> = {
    skill: input.skill,
    input: input.input,
    evals: input.evals.map((e) => e.json),
  };
  if (input.name !== undefined) payload.name = input.name;
  if (input.user !== undefined) payload.user = compileUser(input.user);
  const decls = compileCaseMocks(input.mocks ?? []);
  if (decls.length > 0) payload.mocks = decls;
  if (input.spy || (input.mocks && input.mocks.length > 0)) payload.spy = true;
  return payload;
}

function compileUser(u: SimulatedUser): Record<string, unknown> {
  const json: Record<string, unknown> = { persona: u.persona };
  if (u.doneWhen !== undefined) json.done_when = u.doneWhen;
  if (u.maxTurns !== undefined) json.max_turns = u.maxTurns;
  return json;
}

function compileWhere(where: WhereCriteria): Record<string, unknown> {
  return Object.fromEntries(
    Object.entries(where).map(([key, value]) => [key, compileWhereCriterion(key, value)]),
  );
}

// A case's `called`/`notCalled` `where` runs hook-side (Rust), so only exact
// strings and the shipped matchers can cross — mirror the mock rule.
function compileWhereCriterion(key: string, criterion: Criterion): unknown {
  if (typeof criterion === "string") return criterion;
  if (
    typeof criterion === "object" &&
    criterion !== null &&
    "kind" in criterion &&
    criterion.kind === "matcher" &&
    criterion.compiled !== undefined
  ) {
    return criterion.compiled;
  }
  throw new SkilltestUsageError(
    `eval \`where\` field \`${key}\` must be an exact string, contains(), or matching() — arbitrary predicates cannot run hook-side`,
  );
}

function compileCaseMocks(mocks: readonly ToolSpy[]): Record<string, unknown>[] {
  const decls: Record<string, unknown>[] = [];
  mocks.forEach((mock, index) => {
    if (mock instanceof ToolMock) {
      decls.push(mock.decl(mock.mockName ?? `__case_mock_${index}`));
    } else if (mock.mockName !== undefined) {
      decls.push(mock.caseDecl(mock.mockName));
    }
  });
  return decls;
}
