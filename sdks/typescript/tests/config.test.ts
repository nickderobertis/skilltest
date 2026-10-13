import { execFileSync } from "node:child_process";
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import ts from "typescript";
import { expect, it } from "vitest";
import { type ConfigOverrides, SkilltestUsageError, runSkill, streamSkill } from "../src/index.js";
import { FAKE_PROVIDER, FIXTURES, REPO_ROOT, caseFile } from "./helpers.js";

it("reaches every schema leaf through generated partial types and the public SDK", async () => {
  const scratch = mkdtempSync(join(tmpdir(), "skilltest-config-ts-"));
  const oldProvider = process.env.SKILLTEST_PROVIDER;
  Reflect.deleteProperty(process.env, "SKILLTEST_PROVIDER");
  try {
    const boundary = join(scratch, "oneharness");
    writeFileSync(
      boundary,
      `#!/bin/sh\ncat >/dev/null\nprintf '%s\\n' '{"results":[{"status":"ok","text":"hello"}]}'\n`,
    );
    chmodSync(boundary, 0o755);
    const casePath = join(scratch, "case.json");
    writeFileSync(
      casePath,
      JSON.stringify({
        skill: join(FIXTURES, "skills/greeter"),
        input: "hello",
        mocks: [{ name: "probe", match: { tool: "never" } }],
        evals: [{ type: "not_called", mock: "probe" }],
      }),
    );
    const rows: [string, unknown, Record<string, Record<string, unknown>>][] = JSON.parse(
      execFileSync(
        "python3",
        [
          join(FIXTURES, "contract/config_surfaces.py"),
          join(REPO_ROOT, "schemas/config.schema.json"),
        ],
        { encoding: "utf8" },
      ),
    );
    const literals: string[] = [];
    for (const [path, specimen, contexts] of rows) {
      let value = specimen;
      const payload: Record<string, unknown> = {
        provider: { kind: "oneharness", bin: boundary, history: false },
      };
      for (const [prefix, context] of Object.entries(contexts)) {
        if (!prefix) continue;
        let cursor = payload;
        const parts = prefix.split(".");
        for (const part of parts.slice(0, -1)) {
          cursor[part] ??= {};
          cursor = cursor[part] as Record<string, unknown>;
        }
        cursor[parts.at(-1) as string] = { ...context };
      }
      const provider = payload.provider as Record<string, unknown>;
      if (provider.kind === "command") provider.command = [FAKE_PROVIDER];
      else provider.bin = boundary;
      if (path === "provider.bin") value = boundary;
      if (path === "provider.command") value = [FAKE_PROVIDER];
      if (path === "models" || path === "platforms") value = ["surface"];
      let cursor = payload;
      const parts = path.split(".");
      for (const part of parts.slice(0, -1)) {
        cursor[part] ??= {};
        cursor = cursor[part] as Record<string, unknown>;
      }
      cursor[parts.at(-1) as string] = value;
      literals.push(
        `const example${literals.length}: ConfigOverrides = ${JSON.stringify(payload)};`,
      );
      expect(
        (await runSkill(casePath, { configOverrides: payload as ConfigOverrides })).passed,
        path,
      ).toBe(true);
    }
    // Compile real object literals, so a missing generated leaf fails the test
    // even though TypeScript types disappear at runtime.
    const proof = join(scratch, "proof.ts");
    writeFileSync(
      proof,
      `import type { ConfigOverrides } from ${JSON.stringify(join(REPO_ROOT, "sdks/typescript/src/generated/config_overrides.js"))};\n${literals.join("\n")}`,
    );
    const program = ts.createProgram([proof], {
      strict: true,
      noEmit: true,
      skipLibCheck: true,
      target: ts.ScriptTarget.ES2022,
      module: ts.ModuleKind.NodeNext,
      moduleResolution: ts.ModuleResolutionKind.NodeNext,
    });
    expect(
      ts
        .getPreEmitDiagnostics(program)
        .map((d) => ts.flattenDiagnosticMessageText(d.messageText, "\n")),
    ).toEqual([]);
  } finally {
    if (oldProvider !== undefined) process.env.SKILLTEST_PROVIDER = oldProvider;
    rmSync(scratch, { recursive: true, force: true });
  }
});

it("shares overrides across buffered and streaming calls and retains zero", async () => {
  const configOverrides: ConfigOverrides = { spy: false, mocks: [], judge: null, max_turns: 2 };
  expect((await runSkill(caseFile("greet_pass.yaml"), { configOverrides })).passed).toBe(true);
  const stream = streamSkill(caseFile("greet_pass.yaml"), { configOverrides });
  for await (const _event of stream) {
    /* consume through the real CLI */
  }
  expect(stream.report?.passed).toBe(true);
  await expect(
    runSkill(caseFile("greet_pass.yaml"), { configOverrides: { max_turns: 0 } }),
  ).rejects.toBeInstanceOf(SkilltestUsageError);
});
