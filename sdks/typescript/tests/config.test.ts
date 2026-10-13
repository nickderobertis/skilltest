import { execFileSync } from "node:child_process";
import { chmodSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import ts from "typescript";
import { expect, it } from "vitest";
import { type ConfigOverrides, SkilltestUsageError, runSkill, streamSkill } from "../src/index.js";
import { FAKE_PROVIDER, FIXTURES, REPO_ROOT, SKILLTEST_BIN, caseFile } from "./helpers.js";

it("reaches every schema leaf through generated partial types and the public SDK", async () => {
  const scratch = mkdtempSync(join(tmpdir(), "skilltest-config-ts-"));
  const oldProvider = process.env.SKILLTEST_PROVIDER;
  const oldOneharness = process.env.SKILLTEST_ONEHARNESS_BIN;
  Reflect.deleteProperty(process.env, "SKILLTEST_PROVIDER");
  try {
    const boundary = join(scratch, "oneharness");
    writeFileSync(
      boundary,
      `#!/bin/sh\ncat >/dev/null\nprintf '%s\\n' '{"results":[{"status":"ok","text":"hello"}]}'\n`,
    );
    chmodSync(boundary, 0o755);
    const capture = join(scratch, "argv.json");
    const wrapper = join(scratch, "capture-cli");
    writeFileSync(
      wrapper,
      `#!/usr/bin/env node
const fs = require("node:fs");
const child = require("node:child_process");
fs.writeFileSync(process.env.SDK_CAPTURE_PATH, JSON.stringify(process.argv.slice(2)));
const result = child.spawnSync(process.env.SDK_REAL_CLI, process.argv.slice(2), {stdio:"inherit"});
process.exit(result.status ?? 3);
`,
    );
    chmodSync(wrapper, 0o755);
    process.env.SDK_CAPTURE_PATH = capture;
    process.env.SDK_REAL_CLI = SKILLTEST_BIN;
    process.env.SKILLTEST_ONEHARNESS_BIN = boundary;

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
      // Schema fixtures guarantee an object provider and nonempty property paths.
      // Each cursor is initialized as an object before the assertions below.
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
        (await runSkill(casePath, { bin: wrapper, configOverrides: payload as ConfigOverrides }))
          .passed,
        path,
      ).toBe(true);
      const captured: string[] = JSON.parse(readFileSync(capture, "utf8"));
      const delivered: Record<string, unknown> = {};
      for (let index = 0; index < captured.length; index++) {
        if (captured[index] !== "--set") continue;
        const assignment = captured[index + 1];
        if (!assignment) throw new Error("--set has no value");
        const boundary = assignment.indexOf("=");
        delivered[assignment.slice(0, boundary)] = JSON.parse(assignment.slice(boundary + 1));
      }
      expect(delivered, path).toEqual(payload);
      // The wrapper runs the real CLI; success proves the names are accepted.
      // The argv equality above proves SDK delivery. Core's differing-value
      // leaf suite separately proves application by the resolver.
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
    Reflect.deleteProperty(process.env, "SDK_CAPTURE_PATH");
    Reflect.deleteProperty(process.env, "SDK_REAL_CLI");
    if (oldOneharness !== undefined) process.env.SKILLTEST_ONEHARNESS_BIN = oldOneharness;
    else Reflect.deleteProperty(process.env, "SKILLTEST_ONEHARNESS_BIN");
    rmSync(scratch, { recursive: true, force: true });
  }
});

it("shares overrides across buffered and streaming calls and retains zero", async () => {
  const configOverrides: ConfigOverrides = { spy: false, mocks: [], judge: null, max_turns: 2 };
  expect((await runSkill(caseFile("greet_pass.yaml"), { configOverrides })).passed).toBe(true);
  const stream = streamSkill(caseFile("greet_pass.yaml"), { configOverrides });
  for await (const _event of stream) {
    /* The terminal report arrives only after this event stream drains. */
  }
  expect(stream.report?.passed).toBe(true);
  await expect(
    runSkill(caseFile("greet_pass.yaml"), { configOverrides: { max_turns: 0 } }),
  ).rejects.toBeInstanceOf(SkilltestUsageError);
});

it("routes API judge overrides through HTTP in both run modes", async () => {
  const requests: { model: string; response_format?: unknown }[] = [];
  const server = createServer((request, response) => {
    let body = "";
    request.on("data", (chunk) => {
      body += chunk;
    });
    request.on("end", () => {
      expect(request.url).toBe("/judge");
      expect(request.headers.authorization).toBe("Bearer fixture-key");
      requests.push(JSON.parse(body));
      response.setHeader("Content-Type", "application/json");
      response.end(
        JSON.stringify({
          choices: [
            {
              message: {
                content: '{"value":false} reconsider {"value":true,"reason":"revised API verdict"}',
              },
            },
          ],
          usage: { prompt_tokens: 11, completion_tokens: 2 },
        }),
      );
    });
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  if (!address || typeof address === "string") throw new Error("HTTP boundary has no port");
  process.env.CONFIG_TEST_API_KEY = "fixture-key";
  const configOverrides: ConfigOverrides = {
    judge: {
      kind: "api",
      vendor: "openai",
      api_key_env: "CONFIG_TEST_API_KEY",
      base_url: `http://127.0.0.1:${address.port}/judge`,
      timeout_secs: 10,
      curl_bin: "curl",
      strict_json: false,
    },
    judge_model: "ts-api-model",
  };
  try {
    const report = await runSkill(caseFile("greet_pass.yaml"), { configOverrides });
    expect(report.passed).toBe(true);
    expect(report.runs[0]?.usage?.input_tokens).toBe(22);
    const stream = streamSkill(caseFile("greet_pass.yaml"), { configOverrides });
    for await (const _event of stream) {
      /* The terminal report arrives only after this event stream drains. */
    }
    expect(stream.report?.passed).toBe(true);
    expect(requests).toHaveLength(4);
    expect(
      requests.every(
        (request) => request.model === "ts-api-model" && request.response_format === undefined,
      ),
    ).toBe(true);
    const fallback = await runSkill(caseFile("greet_pass.yaml"), {
      configOverrides,
      models: ["fallback-model"],
      judgeModel: "",
    });
    expect(fallback.passed).toBe(true);
    expect(requests.slice(4).map((request) => request.model)).toEqual([
      "fallback-model",
      "fallback-model",
    ]);
  } finally {
    Reflect.deleteProperty(process.env, "CONFIG_TEST_API_KEY");
    await new Promise<void>((resolve, reject) =>
      server.close((error) => (error ? reject(error) : resolve())),
    );
  }
});
