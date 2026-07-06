/**
 * Stream a running skilltest case as an async iterable of tool events.
 *
 * Opt-in on top of the buffered {@link runSkill}: iterate a {@link SkillStream}
 * with `for await` to receive each normalized tool event the instant the skill
 * takes it, and `break` to **short-circuit** — the CLI subprocess is killed,
 * which closes oneharness's stream and tears the harness down, so a bad turn is
 * cut off instead of paid for in full. After the stream completes normally,
 * `.report` holds the final {@link Report}.
 *
 * ```ts
 * const stream = streamSkill("cases/edit.skilltest.yaml");
 * for await (const ev of stream) {
 *   if (ev.event.name === "bash" && String(ev.event.input).includes("rm -rf")) {
 *     break; // disallowed action — abort the run now
 *   }
 * }
 * const report = stream.report; // the full Report, when it ran to completion
 * ```
 */
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { SkilltestProviderError } from "./errors.js";
import type { Report, ToolEvent } from "./generated/report.js";
import { ENV_BIN, type RunOptions, buildRunArgs, raiseForCode, resolveBin } from "./runner.js";

/** One streamed tool event, tagged with the run it belongs to. */
export interface StreamEvent {
  case: string;
  platform: string;
  model: string;
  /** 1-based assistant-turn index within the run. */
  turn: number;
  event: ToolEvent;
}

/**
 * An async iterable of {@link StreamEvent}s from a running case. Iterate with
 * `for await`; `break` to short-circuit. When the stream runs to completion,
 * `report` holds the final {@link Report}.
 */
export interface SkillStream extends AsyncIterable<StreamEvent> {
  readonly report: Report | undefined;
}

class SkillStreamImpl implements SkillStream {
  report: Report | undefined;

  constructor(
    private readonly bin: string,
    private readonly args: string[],
    private readonly cwd: string | undefined,
  ) {}

  async *[Symbol.asyncIterator](): AsyncIterator<StreamEvent> {
    const child = spawn(this.bin, this.args, { cwd: this.cwd });
    let spawnError: Error | undefined;
    child.on("error", (err) => {
      spawnError = err;
    });
    let stderr = "";
    child.stderr.on("data", (chunk: Buffer) => {
      stderr += chunk.toString();
    });
    const rl = createInterface({ input: child.stdout });
    try {
      for await (const line of rl) {
        const trimmed = line.trim();
        if (!trimmed) continue;
        const obj = JSON.parse(trimmed);
        if (obj.type === "event") {
          yield obj as StreamEvent;
        } else if (obj.type === "result") {
          this.report = obj.report as Report;
        }
      }
      const code = await new Promise<number | null>((resolve) => {
        if (child.exitCode !== null || spawnError) resolve(child.exitCode);
        else child.on("close", resolve);
      });
      if (spawnError) {
        throw new SkilltestProviderError(
          `could not run skilltest binary \`${this.bin}\`: ${spawnError.message}. ` +
            `Set ${ENV_BIN} or pass bin.`,
        );
      }
      raiseForCode(code, stderr.trim());
    } finally {
      // The consumer stopped early (break): kill the CLI so oneharness's stream
      // closes and the harness is torn down.
      rl.close();
      if (child.exitCode === null && !spawnError) child.kill();
    }
  }
}

/**
 * Start a streaming run and return a {@link SkillStream} to iterate. Same options
 * as {@link runSkill}; the run does not begin until iteration starts.
 */
export function streamSkill(casePath: string, options: RunOptions = {}): SkillStream {
  const args = buildRunArgs(casePath, options, "json-stream");
  return new SkillStreamImpl(resolveBin(options.bin), args, options.cwd);
}
