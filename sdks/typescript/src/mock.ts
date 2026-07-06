/**
 * Tool mocking and spying: hold a mock/spy as a variable, pass it to
 * {@link import("./runner.js").runSkill | runSkill}, and assert on it directly —
 * `vi.fn()` semantics over real harness tool calls.
 *
 * ```ts
 * import { runSkill, spy, stub, deny, contains, matching } from "@skill-test/sdk";
 *
 * const git = spy({ tool: "bash", pattern: /\bgit\b/ });           // observe only
 * const push = stub({ contains: "git push", output: "Everything up-to-date" });
 *
 * const report = await runSkill("cases/deploy.skilltest.yaml", { mocks: [push, git] });
 *
 * expect(push.callCount).toBe(1);
 * expect(push.calls[0].input).toMatchObject({ command: expect.stringContaining("origin") });
 * expect(git.where({ command: matching(/\bsudo\b/) }).called).toBe(false);
 * ```
 *
 * Vocabulary (sinon's): a **spy** observes without intercepting; a **stub**
 * fakes a shell call's result; **deny** blocks with a model-visible message;
 * **rewrite** substitutes raw input fields. {@link ToolMock} extends
 * {@link ToolSpy}, so every mock carries the full spy surface.
 *
 * Two matching engines, on purpose: a **mock's** criteria compile into the
 * hook-side ruleset that runs inside the harness (`pattern` is Rust-regex — no
 * lookarounds — and `where` values must be exact strings or the shipped
 * matchers), while a **spy** filters the returned records locally, so its
 * `pattern` is a native `RegExp` and `where` accepts arbitrary predicates.
 *
 * These are deliberately *not* `vi.fn()` mocks: tool calls are structured
 * records (tool, input, platform/model, verdict), not argument tuples, and the
 * useful assertions are partial/fuzzy. `.calls` is plain typed data, so every
 * `expect` idiom works on it directly. Reading an unbound spy throws instead of
 * counting as zero calls.
 */
import { SkilltestUsageError } from "./errors.js";
import type { CaseRun } from "./generated/report.js";

/** A predicate on one input-field value; build with {@link contains} /
 * {@link matching} / {@link anything}. `compiled` is the hook-side predicate a
 * *mock's* criteria lower to (`undefined` when it cannot cross that boundary). */
export interface Matcher {
  readonly kind: "matcher";
  readonly description: string;
  readonly compiled?: Record<string, string>;
  matches(value: string): boolean;
}

/** What a `where` criterion value may be: an exact scalar (non-strings compare
 * by their JSON form, mirroring the hook-side coercion), a {@link Matcher}, or
 * an arbitrary predicate on the field's string form. */
export type Criterion = string | number | boolean | null | Matcher | ((value: string) => boolean);

/** Per-field criteria on a call's input; the reserved `tool` key (in
 * {@link ToolSpy.where} only) targets the tool name instead. */
export type WhereCriteria = Record<string, Criterion>;

/** Match a field whose value contains `needle`. */
export function contains(needle: string): Matcher {
  if (!needle) {
    throw new SkilltestUsageError("contains() needs a non-empty needle (empty matches everything)");
  }
  return {
    kind: "matcher",
    description: `contains(${JSON.stringify(needle)})`,
    compiled: { contains: needle },
    matches: (value) => value.includes(needle),
  };
}

/**
 * Match a field whose value matches `pattern` (unanchored). In a spy (local
 * matching) this is a native `RegExp`; compiled into a mock it runs as
 * Rust-regex inside the harness — stick to the shared subset (no lookarounds)
 * for criteria used both ways.
 */
export function matching(pattern: string | RegExp): Matcher {
  const re = typeof pattern === "string" ? new RegExp(pattern) : pattern;
  if (!re.source || re.source === "(?:)") {
    throw new SkilltestUsageError(
      "matching() needs a non-empty pattern (empty matches everything)",
    );
  }
  return {
    kind: "matcher",
    description: `matching(/${re.source}/)`,
    compiled: { pattern: re.source },
    matches: (value) => re.test(value),
  };
}

/** Match any value — asserts the field merely *exists* on the call. Spy
 * (local) matching only; the hook-side ruleset has no exists-predicate. */
export function anything(): Matcher {
  return {
    kind: "matcher",
    description: "anything()",
    matches: () => true,
  };
}

/**
 * One observed tool call, with its **original** (pre-rewrite) input and the
 * run it belongs to. The transcript's `events` show post-rewrite reality (the
 * stub that actually ran); this shows what the skill *attempted*.
 */
export interface ToolCall {
  readonly tool: string | null;
  /** The tool's structured input arguments (harness-specific shape). */
  readonly input: Record<string, unknown> | null;
  /** The verdict applied: `allow`, `deny`, `rewrite`, or `stub`. */
  readonly action: string;
  /** Name of the intercepting mock, when one did. */
  readonly mock: string | null;
  readonly platform: string;
  readonly model: string;
  /** True iff a mock's verdict was applied to this call. */
  readonly mocked: boolean;
  /** Sugar for the shell 90%-case: `input.command` when it is a string. */
  readonly command: string | null;
}

/** Criteria a spy/mock is constructed from. */
export interface MatchOptions {
  /** Case-insensitive exact match on the (per-harness) tool name. */
  tool?: string;
  /** Substring match over the call's event JSON. */
  contains?: string;
  /** Regex over the same haystack (see {@link matching} for engine notes). */
  pattern?: string | RegExp;
  /** Per-input-field criteria (see {@link WhereCriteria}). */
  where?: WhereCriteria;
}

function coerceField(value: unknown): string {
  return typeof value === "string" ? value : JSON.stringify(value);
}

function criterionHolds(criterion: Criterion, value: unknown): boolean {
  const text = coerceField(value);
  if (typeof criterion === "function") return criterion(text);
  if (typeof criterion === "object" && criterion !== null && criterion.kind === "matcher") {
    return criterion.matches(text);
  }
  if (typeof criterion === "string") return text === criterion;
  // Non-string scalars compare by their JSON form, mirroring the field coercion.
  return text === JSON.stringify(criterion);
}

function whereHolds(criteria: WhereCriteria, call: ToolCall): boolean {
  for (const [key, criterion] of Object.entries(criteria)) {
    if (call.input === null || !(key in call.input)) return false;
    if (!criterionHolds(criterion, call.input[key])) return false;
  }
  return true;
}

function summarize(call: ToolCall): string {
  const detail = call.command ?? coerceField(call.input);
  const base = `${call.tool ?? "?"}(${detail})`;
  return call.mocked ? `${base} [${call.action}]` : base;
}

function describeObserved(records: readonly ToolCall[]): string {
  if (records.length === 0) return "";
  const summaries = records.slice(0, 5).map(summarize).join(", ");
  const more = records.length > 5 ? `, … ${records.length - 5} more` : "";
  return `; observed: ${summaries}${more}`;
}

/**
 * A spy on tool calls: observes every matching call without intercepting.
 * Construct with {@link spy}, pass it in `mocks` to
 * {@link import("./runner.js").runSkill | runSkill}, then assert on the typed
 * properties. Unbound access throws — "no run yet" must never read as "zero
 * calls". Each run re-binds fresh.
 */
export class ToolSpy {
  protected readonly criteria: MatchOptions;
  protected bound: ToolCall[] | undefined;
  /** For a `where()` view: the parent's calls, shown in failure context when
   * the filtered result is empty. */
  protected pool: ToolCall[] | undefined;

  constructor(criteria: MatchOptions) {
    const { tool, contains: needle, pattern, where } = criteria;
    if (
      tool === undefined &&
      needle === undefined &&
      pattern === undefined &&
      (where === undefined || Object.keys(where).length === 0)
    ) {
      throw new SkilltestUsageError(
        "a spy/mock needs at least one criterion (tool, contains, pattern, where)",
      );
    }
    if (tool === "" || needle === "") {
      throw new SkilltestUsageError("an empty criterion would match everything");
    }
    if (pattern !== undefined) {
      const source = typeof pattern === "string" ? pattern : pattern.source;
      if (!source || source === "(?:)") {
        throw new SkilltestUsageError("an empty `pattern` would match everything");
      }
    }
    this.criteria = criteria;
  }

  /** @internal Attach a run's matching calls; called by the runner. */
  bind(calls: readonly ToolCall[]): void {
    this.bound = calls.filter((call) => this.matches(call));
  }

  protected matches(call: ToolCall): boolean {
    const { tool, contains: needle, pattern, where } = this.criteria;
    if (tool !== undefined) {
      if (call.tool === null || call.tool.toLowerCase() !== tool.toLowerCase()) return false;
    }
    if (needle !== undefined || pattern !== undefined) {
      // The same haystack the hook-side rules match: the compact event JSON
      // carrying the tool name and its input.
      const event: Record<string, unknown> = {};
      if (call.tool !== null) event.tool_name = call.tool;
      if (call.input !== null) event.tool_input = call.input;
      const haystack = JSON.stringify(event);
      if (needle !== undefined && !haystack.includes(needle)) return false;
      if (pattern !== undefined) {
        const re = typeof pattern === "string" ? new RegExp(pattern) : pattern;
        if (!re.test(haystack)) return false;
      }
    }
    return where === undefined || whereHolds(where, call);
  }

  /** The matching observed calls, in order. Throws when unbound. */
  get calls(): ToolCall[] {
    if (this.bound === undefined) {
      throw new SkilltestUsageError(
        "this spy/mock is not bound to a run yet — pass it to runSkill({ mocks: [...] }) " +
          "and read it after the run completes",
      );
    }
    return [...this.bound];
  }

  /** True iff at least one matching call was observed. */
  get called(): boolean {
    return this.calls.length > 0;
  }

  get callCount(): number {
    return this.calls.length;
  }

  /**
   * A filtered view with the identical surface. Keys target input fields
   * (values: exact scalar, {@link contains}/{@link matching}/{@link anything},
   * or any predicate); the reserved `tool` key targets the tool name. An empty
   * result is a full citizen — `expect(view.called).toBe(false)` composes.
   * A miss's context (`observed`) falls back to the parent's calls.
   */
  where(criteria: WhereCriteria): ToolSpy {
    const { tool, ...fields } = criteria;
    const filtered = this.calls.filter((call) => {
      if (tool !== undefined) {
        if (call.tool === null || !criterionHolds(tool, call.tool)) return false;
      }
      return whereHolds(fields, call);
    });
    const view = Object.create(ToolSpy.prototype) as ToolSpy & {
      bound: ToolCall[];
      pool: ToolCall[];
      criteria: MatchOptions;
    };
    view.criteria = {};
    view.bound = filtered;
    view.pool = this.calls;
    return view;
  }

  /** The observed calls for assertion-failure context: this spy's own, or —
   * for an empty `where()` view — its parent's. */
  get observed(): string {
    const records = this.bound?.length ? this.bound : (this.pool ?? []);
    return describeObserved(records);
  }
}

/** One mock action, keyed exactly like the YAML declaration. */
type MockAction =
  | { stub: { output: string; exit_code: number } }
  | { deny: string }
  | { rewrite: Record<string, unknown> };

/**
 * A mock: a spy that also **intercepts** — its criteria compile into the
 * hook-side ruleset, so matching happens inside the harness. Construct with
 * {@link stub} / {@link deny} / {@link rewrite}.
 */
export class ToolMock extends ToolSpy {
  private readonly action: MockAction;
  /** Synthetic declaration name, assigned when compiled into a run. */
  private name: string | undefined;

  constructor(action: MockAction, criteria: MatchOptions) {
    super(criteria);
    this.action = action;
  }

  /** @internal The declaration this compiles to in the `--mocks` file. */
  decl(name: string): Record<string, unknown> {
    this.name = name;
    const match: Record<string, unknown> = {};
    if (this.criteria.tool !== undefined) match.tool = this.criteria.tool;
    if (this.criteria.contains !== undefined) match.contains = this.criteria.contains;
    if (this.criteria.pattern !== undefined) {
      match.pattern =
        typeof this.criteria.pattern === "string"
          ? this.criteria.pattern
          : this.criteria.pattern.source;
    }
    if (this.criteria.where !== undefined && Object.keys(this.criteria.where).length > 0) {
      match.input = Object.fromEntries(
        Object.entries(this.criteria.where).map(([key, criterion]) => [
          key,
          compileCriterion(key, criterion),
        ]),
      );
    }
    return { name, match, ...this.action };
  }

  /** @internal A mock binds the calls its own rule intercepted (by resolved
   * name), not a local re-match — the hook's decision is the truth. */
  override bind(calls: readonly ToolCall[]): void {
    this.bound = calls.filter((call) => call.mock !== null && call.mock === this.name);
  }
}

/**
 * Compile a factory `where` criterion into the hook-side predicate shape. Only
 * exact strings and the shipped matchers can cross the boundary — arbitrary
 * predicates cannot run inside the harness, so they are a loud
 * construction-time error (use a spy for those).
 */
function compileCriterion(key: string, criterion: Criterion): Record<string, string> | string {
  if (typeof criterion === "string") return criterion;
  if (
    typeof criterion === "object" &&
    criterion !== null &&
    criterion.kind === "matcher" &&
    criterion.compiled !== undefined
  ) {
    return criterion.compiled;
  }
  throw new SkilltestUsageError(
    `mock criterion \`${key}\` cannot be compiled into the hook-side ruleset; mocks accept exact strings, contains(), or matching() — use a spy for arbitrary predicates`,
  );
}

/**
 * A spy: observe every matching tool call, intercept nothing. Spies filter
 * locally, so `pattern` is a native `RegExp` and `where` values may be
 * arbitrary predicates.
 */
export function spy(criteria: MatchOptions): ToolSpy {
  return new ToolSpy(criteria);
}

/**
 * Fake a matching SHELL call's result: the real command never runs and the
 * model receives `output` as the tool's genuine result. `pattern` is
 * Rust-regex (linear-time; no lookarounds) — it runs inside the harness.
 */
export function stub(options: MatchOptions & { output: string; exitCode?: number }): ToolMock {
  const { output, exitCode, ...criteria } = options;
  return new ToolMock({ stub: { output, exit_code: exitCode ?? 0 } }, criteria);
}

/**
 * Block a matching call; the model reads `message` as the tool's feedback.
 * Works on every hook-capable harness (the most portable verb).
 */
export function deny(options: MatchOptions & { message: string }): ToolMock {
  const { message, ...criteria } = options;
  return new ToolMock({ deny: message }, criteria);
}

/**
 * Substitute a matching call's raw input fields — the low-level escape hatch,
 * and the way to mock file reads (rewrite `file_path` to a fixture). `input`
 * is the substituted arguments object.
 */
export function rewrite(options: MatchOptions & { input: Record<string, unknown> }): ToolMock {
  const { input, ...criteria } = options;
  return new ToolMock({ rewrite: input }, criteria);
}

/**
 * @internal The `--mocks` declaration list for a run: one entry per
 * {@link ToolMock} with a synthetic name; spies contribute nothing (they
 * filter locally) but still require the channel (`--spy`).
 */
export function compileDecls(mocks: readonly ToolSpy[]): Record<string, unknown>[] {
  const decls: Record<string, unknown>[] = [];
  mocks.forEach((mock, index) => {
    if (mock instanceof ToolMock) decls.push(mock.decl(`__mock_${index}`));
  });
  return decls;
}

/**
 * @internal Bind every spy/mock to the report's records. Loud when a run
 * carries no channel — a spy must never silently read as zero calls.
 */
export function bindMocks(mocks: readonly ToolSpy[], runs: readonly CaseRun[]): void {
  const calls: ToolCall[] = [];
  for (const run of runs) {
    const records = run.mock_calls;
    if (records === undefined || records === null) {
      throw new SkilltestUsageError(
        `run \`${run.case}\` [${run.platform}/${run.model}] reported no mock/spy observations; the provider/platform does not support the mock channel`,
      );
    }
    for (const record of records) {
      const input = (record.input ?? null) as Record<string, unknown> | null;
      const command = input !== null && typeof input.command === "string" ? input.command : null;
      calls.push({
        tool: record.tool ?? null,
        input,
        action: record.action,
        mock: record.mock ?? null,
        platform: run.platform,
        model: run.model,
        mocked: record.action !== "allow",
        command,
      });
    }
  }
  for (const mock of mocks) mock.bind(calls);
}
