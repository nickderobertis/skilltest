/**
 * Hand-written conveniences over the generated contract types. The type
 * checker keeps these honest against `generated/` — a renamed or removed
 * field fails `tsc`, not a user's test.
 */
import type { Report, Transcript } from "./generated/report.js";

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
