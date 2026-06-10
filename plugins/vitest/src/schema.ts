/**
 * Zod schemas mirroring the `skilltest --format json` contract.
 *
 * Every payload crossing the process boundary from the CLI is parsed through
 * these schemas before any test code touches it, so a contract drift surfaces as
 * a clear validation error instead of an `undefined` deep in a test.
 */
import { z } from "zod";

export const MessageSchema = z.object({
  role: z.enum(["user", "assistant", "system"]),
  content: z.string(),
});

export const TranscriptSchema = z.object({
  messages: z.array(MessageSchema),
});

/**
 * Token / cost usage aggregated for a run or the whole report.
 * Each field is independently nullable because not every harness reports
 * every signal (cost is commonly absent on subscription auth).
 */
export const UsageSchema = z.object({
  input_tokens: z.number().optional(),
  output_tokens: z.number().optional(),
  cost_usd: z.number().optional(),
});

export const BooleanDetailSchema = z.object({
  kind: z.literal("boolean"),
  value: z.boolean(),
  expected: z.boolean(),
});

export const NumericDetailSchema = z.object({
  kind: z.literal("numeric"),
  value: z.number(),
  threshold: z.number(),
  comparator: z.string(),
});

export const EvalDetailSchema = z.discriminatedUnion("kind", [
  BooleanDetailSchema,
  NumericDetailSchema,
]);

export const EvalOutcomeSchema = z.object({
  label: z.string(),
  passed: z.boolean(),
  detail: EvalDetailSchema,
  reason: z.string(),
});

export const CaseRunSchema = z.object({
  case: z.string(),
  skill: z.string(),
  platform: z.string(),
  model: z.string(),
  passed: z.boolean(),
  turns: z.number(),
  evals: z.array(EvalOutcomeSchema),
  transcript: TranscriptSchema,
  usage: UsageSchema.optional(),
});

export const SummarySchema = z.object({
  cases: z.number(),
  runs: z.number(),
  passed: z.number(),
  failed: z.number(),
  usage: UsageSchema.optional(),
});

export const ReportSchema = z.object({
  passed: z.boolean(),
  summary: SummarySchema,
  runs: z.array(CaseRunSchema),
});

export const ValidationFindingSchema = z.object({
  skill: z.string(),
  message: z.string(),
});

export const ValidationReportSchema = z.object({
  valid: z.boolean(),
  findings: z.array(ValidationFindingSchema),
});

export type Message = z.infer<typeof MessageSchema>;
export type Transcript = z.infer<typeof TranscriptSchema>;
export type Usage = z.infer<typeof UsageSchema>;
export type EvalDetail = z.infer<typeof EvalDetailSchema>;
export type EvalOutcome = z.infer<typeof EvalOutcomeSchema>;
export type CaseRun = z.infer<typeof CaseRunSchema>;
export type Summary = z.infer<typeof SummarySchema>;
export type Report = z.infer<typeof ReportSchema>;
export type ValidationFinding = z.infer<typeof ValidationFindingSchema>;
export type ValidationReport = z.infer<typeof ValidationReportSchema>;

/** The assistant turns of a transcript joined — handy for mix-in checks. */
export function assistantText(transcript: Transcript): string {
  return transcript.messages
    .filter((m) => m.role === "assistant")
    .map((m) => m.content)
    .join("\n");
}

/** A one-line-per-failed-eval summary, for assertion messages. */
export function describeFailures(report: Report): string {
  const lines: string[] = [];
  for (const run of report.runs) {
    if (run.passed) continue;
    for (const outcome of run.evals) {
      if (!outcome.passed) {
        lines.push(
          `${run.case} [${run.platform}/${run.model}] ${outcome.label}: ${outcome.reason}`,
        );
      }
    }
  }
  return lines.join("\n");
}
