/**
 * Contract tests: the SDK's Zod schemas must stay in sync with the CLI's JSON
 * Schema.
 *
 * The golden schemas in `schemas/` are generated from the Rust report types
 * (`skilltest schema`, regenerated with `just gen-schemas`) and guarded against
 * drift by the Rust e2e suite. Here we compare every Zod schema field-by-field
 * against those goldens, in both directions, so a field added or removed on
 * either side fails loudly with the exact difference.
 */
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import type { z } from "zod";
import {
  BooleanDetailSchema,
  CaseRunSchema,
  ComparatorSchema,
  EvalDetailSchema,
  EvalOutcomeSchema,
  MessageSchema,
  NumericDetailSchema,
  ReportSchema,
  SummarySchema,
  TranscriptSchema,
  UsageSchema,
  ValidationFindingSchema,
  ValidationReportSchema,
} from "../src/index.js";
import { REPO_ROOT } from "./helpers.js";

/** The slice of JSON Schema the goldens use. */
interface Def {
  properties?: Record<string, Def>;
  required?: string[];
  oneOf?: Def[];
  const?: string;
}

type Golden = Def & { $defs: Record<string, Def> };

function loadGolden(name: string): Golden {
  return JSON.parse(readFileSync(join(REPO_ROOT, "schemas", name), "utf8")) as Golden;
}

/**
 * Schema definition name -> the Zod object schema mirroring it. Object schemas
 * only; enums and the tagged union are checked separately below.
 */
const REPORT_DEF_SCHEMAS: Record<string, z.AnyZodObject> = {
  CaseRun: CaseRunSchema,
  EvalOutcome: EvalOutcomeSchema,
  Message: MessageSchema,
  Summary: SummarySchema,
  Transcript: TranscriptSchema,
  Usage: UsageSchema,
};

/** Zod object fields and required-ness must match the schema object exactly. */
function expectObjectMatches(schema: z.AnyZodObject, def: Def, where: string): void {
  const fields = Object.keys(schema.shape).sort();
  const required = fields.filter((key) => {
    const field = schema.shape[key] as z.ZodTypeAny | undefined;
    return field !== undefined && !field.isOptional();
  });
  expect(fields, `${where}: model fields vs schema properties`).toEqual(
    Object.keys(def.properties ?? {}).sort(),
  );
  expect(required, `${where}: required fields`).toEqual([...(def.required ?? [])].sort());
}

/** The `const` values of a schema enum rendered as a oneOf of consts. */
function variantConsts(def: Def): string[] {
  return (def.oneOf ?? [])
    .map((v) => v.const)
    .filter((v): v is string => v !== undefined)
    .sort();
}

describe("report contract", () => {
  const golden = loadGolden("report.schema.json");

  it("covers exactly the types in the golden schema", () => {
    const covered = [...Object.keys(REPORT_DEF_SCHEMAS), "EvalDetail", "Comparator", "Role"];
    expect(
      Object.keys(golden.$defs).sort(),
      "a type was added to or removed from the contract",
    ).toEqual(covered.sort());
  });

  it("matches every object definition field-by-field", () => {
    expectObjectMatches(ReportSchema, golden, "Report");
    for (const [name, schema] of Object.entries(REPORT_DEF_SCHEMAS)) {
      expectObjectMatches(schema, golden.$defs[name] ?? {}, name);
    }
  });

  it("matches the EvalDetail variants", () => {
    const variants = new Map(
      (golden.$defs.EvalDetail?.oneOf ?? []).map((v) => [v.properties?.kind?.const, v]),
    );
    expect([...variants.keys()].sort()).toEqual(["boolean", "numeric"]);
    expectObjectMatches(BooleanDetailSchema, variants.get("boolean") ?? {}, "EvalDetail.boolean");
    expectObjectMatches(NumericDetailSchema, variants.get("numeric") ?? {}, "EvalDetail.numeric");
  });

  it("matches the enums", () => {
    expect([...ComparatorSchema.options].sort()).toEqual(
      variantConsts(golden.$defs.Comparator ?? {}),
    );
    const role = MessageSchema.shape.role;
    expect([...role.options].sort()).toEqual(variantConsts(golden.$defs.Role ?? {}));
  });

  it("parses a report that follows the discriminated union", () => {
    // Belt-and-braces: the union discriminator itself round-trips.
    const detail = EvalDetailSchema.parse({
      kind: "numeric",
      value: 8,
      threshold: 7,
      comparator: "gte",
    });
    expect(detail.kind).toBe("numeric");
  });
});

describe("validation contract", () => {
  const golden = loadGolden("validation.schema.json");

  it("matches every definition field-by-field", () => {
    expectObjectMatches(ValidationReportSchema, golden, "ValidationReport");
    expect(Object.keys(golden.$defs)).toEqual(["ValidationFinding"]);
    expectObjectMatches(
      ValidationFindingSchema,
      golden.$defs.ValidationFinding ?? {},
      "ValidationFinding",
    );
  });
});
