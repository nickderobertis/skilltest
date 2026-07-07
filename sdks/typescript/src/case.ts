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
 * the run, and referenceable from a `called`/`notCalled` eval — pass the
 * object itself, or its `name`), and `evals` decide pass/fail.
 *
 * ```ts
 * const push = stub({ pattern: /git push\b/, output: "Everything up-to-date" });
 * const report = await runSkill(
 *   testCase({
 *     skill: "skills/deployer",
 *     input: "Deploy the app",
 *     mocks: [push],
 *     evals: [called(push, { times: 1 })], // the object, no string name to keep in sync
 *   }),
 * );
 * ```
 *
 * The builders construct types **generated from the CLI's own input schema**
 * (`schemas/case.schema.json` → `src/generated/case.ts`, via `just
 * gen-contract`), so the payload shape cannot drift from the Rust parse: a
 * renamed or removed field is a compile error here, not a silently-ignored key
 * at runtime. The CLI then validates the compiled case semantically, so a
 * malformed one throws a
 * {@link import("./errors.js").SkilltestUsageError | SkilltestUsageError}.
 *
 * Writing cases in code is the recommended approach; YAML files remain fully
 * supported (`runSkill("cases/greet.yaml")`) and are what `discover` collects.
 */
import { SkilltestUsageError } from "./errors.js";
import type {
  BooleanEval,
  CalledEval,
  TestCase as CaseJson,
  Comparator,
  Eval,
  FieldPredicate,
  NotCalledEval,
  NumericEval,
  SimulatedUser,
} from "./generated/case.js";
import { type Criterion, ToolMock, ToolSpy, type WhereCriteria } from "./mock.js";

export type {
  BooleanEval,
  CalledEval,
  Eval,
  NotCalledEval,
  NumericEval,
  SimulatedUser,
} from "./generated/case.js";

/** A `called`/`notCalled` eval holding the referenced {@link ToolSpy}/
 * {@link ToolMock} *object* instead of a string name. Built by passing the
 * object to {@link called}/{@link notCalled}; resolved to the object's
 * compiled declaration name when the case compiles — the object must be in
 * that case's `mocks`. */
export interface MockRefEval {
  type: "called" | "not_called";
  mock: ToolSpy;
  times?: number;
  where?: Record<string, FieldPredicate>;
  name?: string;
}

/** One eval as accepted by a {@link TestCaseInput}: a generated contract
 * eval, or a {@link MockRefEval} (a `called`/`notCalled` holding the spy/mock
 * object itself). */
export type CaseEval = Eval | MockRefEval;

/** A full test case defined in code — the twin of a `*.skilltest.yaml` file.
 * Pass one (via {@link testCase}) to {@link import("./runner.js").runSkill}. */
export interface TestCaseInput {
  /** Skill directory under test (a dir containing SKILL.md), resolved relative
   * to the working directory. */
  skill: string;
  /** The initial data/prompt handed to the skill as the first user message. */
  input: string;
  /** The evals that decide pass/fail (must be non-empty). */
  evals: CaseEval[];
  /** Optional report label (defaults to `case`). */
  name?: string;
  /** Present ⇒ multi-turn (see {@link user}). */
  user?: SimulatedUser;
  /** Mock/spy objects for this case — the same builders `runSkill({ mocks })`
   * takes. Bound for assertions after the run; referenceable from a
   * `called`/`notCalled` eval (pass the object itself, or give it a `name`
   * and reference that). */
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
): BooleanEval {
  return {
    type: "boolean",
    criterion,
    expected: options.expected ?? true,
    ...(options.name !== undefined && { name: options.name }),
  };
}

/** The comparator sugar {@link numeric} accepts: the symbol or its canonical
 * wire name. */
export type ComparatorInput = Comparator | ">=" | ">" | "<=" | "<";

const COMPARATORS: Record<ComparatorInput, Comparator> = {
  ">=": "gte",
  ">": "gt",
  "<=": "lte",
  "<": "lt",
  gte: "gte",
  gt: "gt",
  lte: "lte",
  lt: "lt",
};

/** Score `criterion` on the `[min, max]` scale and pass when the score
 * satisfies `comparator` (`>=` `>` `<=` `<`, default `>=`) against `threshold`. */
export function numeric(
  criterion: string,
  options: {
    min: number;
    max: number;
    threshold: number;
    comparator?: ComparatorInput;
    name?: string;
  },
): NumericEval {
  return {
    type: "numeric",
    criterion,
    min: options.min,
    max: options.max,
    threshold: options.threshold,
    comparator: COMPARATORS[options.comparator ?? ">="],
    ...(options.name !== undefined && { name: options.name }),
  };
}

/** Deterministic (no judge): assert the `mock`/spy observed at least one
 * matching call, or exactly `times`. Pass the spy/stub/… object from the
 * case's `mocks` directly, or reference it by the `name` you gave it. `where`
 * narrows by input field. */
export function called(
  mock: string,
  options?: { times?: number; where?: WhereCriteria; name?: string },
): CalledEval;
export function called(
  mock: ToolSpy,
  options?: { times?: number; where?: WhereCriteria; name?: string },
): MockRefEval;
export function called(
  mock: string | ToolSpy,
  options: { times?: number; where?: WhereCriteria; name?: string } = {},
): CalledEval | MockRefEval {
  const rest = {
    ...(options.times !== undefined && { times: options.times }),
    ...(options.where !== undefined && { where: compileWhere(options.where) }),
    ...(options.name !== undefined && { name: options.name }),
  };
  return typeof mock === "string"
    ? { type: "called", mock, ...rest }
    : { type: "called", mock, ...rest };
}

/** Deterministic: assert the `mock`/spy (the object from the case's `mocks`,
 * or its `name`) observed **no** matching call (optionally narrowed by
 * `where`). */
export function notCalled(
  mock: string,
  options?: { where?: WhereCriteria; name?: string },
): NotCalledEval;
export function notCalled(
  mock: ToolSpy,
  options?: { where?: WhereCriteria; name?: string },
): MockRefEval;
export function notCalled(
  mock: string | ToolSpy,
  options: { where?: WhereCriteria; name?: string } = {},
): NotCalledEval | MockRefEval {
  const rest = {
    ...(options.where !== undefined && { where: compileWhere(options.where) }),
    ...(options.name !== undefined && { name: options.name }),
  };
  return typeof mock === "string"
    ? { type: "not_called", mock, ...rest }
    : { type: "not_called", mock, ...rest };
}

/** A simulated user for a multi-turn case: after each assistant turn the judge
 * plays `persona` until `doneWhen` holds, the skill reports done, or `maxTurns`
 * is reached. */
export function user(
  persona: string,
  options: { doneWhen?: string; maxTurns?: number } = {},
): SimulatedUser {
  return {
    persona,
    ...(options.doneWhen !== undefined && { done_when: options.doneWhen }),
    ...(options.maxTurns !== undefined && { max_turns: options.maxTurns }),
  };
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

/** @internal The JSON the CLI ingests via `--case-json`, typed as the
 * generated case model so the payload shape is pinned to the input contract:
 * assigns names to the case's mocks (so binding and `called`/`notCalled`
 * references resolve — including evals holding the mock/spy *object*) and
 * turns the spy channel on when any mock/spy is present. */
export function compileCase(input: TestCaseInput): CaseJson {
  const referenced = new Set(input.evals.filter(isMockRefEval).map((evalRef) => evalRef.mock));
  const { decls, names } = compileCaseMocks(input.mocks ?? [], referenced);
  return {
    skill: input.skill,
    input: input.input,
    evals: input.evals.map((entry) => resolveEval(entry, names)),
    ...(input.name !== undefined && { name: input.name }),
    ...(input.user !== undefined && { user: input.user }),
    ...(decls.length > 0 && { mocks: decls }),
    ...((input.spy || (input.mocks?.length ?? 0) > 0) && { spy: true }),
  };
}

function isMockRefEval(entry: CaseEval): entry is MockRefEval {
  return (entry.type === "called" || entry.type === "not_called") && entry.mock instanceof ToolSpy;
}

/** The generated eval, with an object reference swapped for its compiled
 * declaration name — which only exists when the object is in the case's own
 * `mocks`, so a forgotten entry is a loud usage error, never a vacuous pass. */
function resolveEval(entry: CaseEval, names: Map<ToolSpy, string>): Eval {
  if (!isMockRefEval(entry)) return entry;
  const resolved = names.get(entry.mock);
  if (resolved === undefined) {
    throw new SkilltestUsageError(
      `a ${entry.type} eval references a spy/mock object that is not in this case's \`mocks\` — pass the same object in the case's mocks array`,
    );
  }
  const rest = {
    ...(entry.where !== undefined && { where: entry.where }),
    ...(entry.name !== undefined && { name: entry.name }),
  };
  return entry.type === "called"
    ? {
        type: "called",
        mock: resolved,
        ...(entry.times !== undefined && { times: entry.times }),
        ...rest,
      }
    : { type: "not_called", mock: resolved, ...rest };
}

function compileWhere(where: WhereCriteria): Record<string, FieldPredicate> {
  return Object.fromEntries(
    Object.entries(where).map(([key, value]) => [key, compileWhereCriterion(key, value)]),
  );
}

// A case's `called`/`notCalled` `where` runs hook-side (Rust), so only exact
// strings and the shipped matchers can cross — mirror the mock rule.
function compileWhereCriterion(key: string, criterion: Criterion): FieldPredicate {
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

/** Compile a case's `mocks` into declarations plus the object→assigned-name
 * map eval object references resolve through: every intercepting mock becomes
 * an action declaration; a **named** spy — or an unnamed one a
 * `called`/`notCalled` eval references — becomes a no-action declaration.
 * Other unnamed spies contribute nothing here (they filter locally after the
 * run) but still turn the channel on via the case's `spy` flag. */
function compileCaseMocks(
  mocks: readonly ToolSpy[],
  referenced: ReadonlySet<ToolSpy>,
): { decls: import("./generated/case.js").MockDecl[]; names: Map<ToolSpy, string> } {
  const decls: import("./generated/case.js").MockDecl[] = [];
  const names = new Map<ToolSpy, string>();
  mocks.forEach((mock, index) => {
    if (mock instanceof ToolMock) {
      const assigned = mock.mockName ?? `__case_mock_${index}`;
      decls.push(mock.decl(assigned));
      names.set(mock, assigned);
    } else if (mock.mockName !== undefined) {
      decls.push(mock.caseDecl(mock.mockName));
      names.set(mock, mock.mockName);
    } else if (referenced.has(mock)) {
      const assigned = `__case_mock_${index}`;
      decls.push(mock.caseDecl(assigned));
      names.set(mock, assigned);
    }
  });
  return { decls, names };
}
