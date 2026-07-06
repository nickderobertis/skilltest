/**
 * Hand-written conveniences over the generated contract types. The type
 * checker keeps these honest against `generated/` — a renamed or removed
 * field fails `tsc`, not a user's test.
 */
import type { Report, ToolEvent, Transcript } from "./generated/report.js";

/** The assistant turns of a transcript joined — handy for mix-in checks. */
export function assistantText(transcript: Transcript): string {
  return transcript.messages
    .filter((m) => m.role === "assistant")
    .map((m) => m.content)
    .join("\n");
}

/**
 * Every `tool_call` event across the transcript's assistant turns, in order.
 *
 * The normalized tool events the skill took (shell commands, file edits, tool
 * uses), lifted from oneharness `--events` — for asserting on *what the skill
 * did*, not just what it said. Empty for harnesses that expose no transcript.
 */
export function toolCalls(transcript: Transcript): ToolEvent[] {
  return transcript.messages.flatMap((m) => m.events ?? []).filter((e) => e.kind === "tool_call");
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
