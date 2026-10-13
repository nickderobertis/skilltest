import { readFileSync } from "node:fs";
import { object, string, strings } from "./validation.mjs";
try {
  if (process.argv.length !== 4)
    throw new Error("usage: npm-identity.mjs <expected-name> <expected-version>");
  const name = string(process.argv[2], "expected npm name");
  const version = string(process.argv[3], "expected version");
  if (
    !/^@skill-test\/cli-(linux|darwin|win32)-(x64|arm64)$/.test(name) ||
    !/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version)
  )
    throw new Error("invalid declared carrier identity");
  const manifest = object(JSON.parse(readFileSync(0, "utf8")), "packed manifest");
  const [, os, cpu] = name.match(/cli-(linux|darwin|win32)-(x64|arm64)$/) ?? [];
  if (
    manifest.name !== name ||
    manifest.version !== version ||
    JSON.stringify(strings(manifest.cpu, "cpu")) !== JSON.stringify([cpu]) ||
    JSON.stringify(strings(manifest.os, "os")) !== JSON.stringify([os])
  )
    throw new Error("packed carrier identity differs from declared target/version");
} catch (error) {
  console.error(
    `npm carrier: ${error instanceof Error ? error.message : String(error)}; restage the declared target before publishing`,
  );
  process.exitCode = 1;
}
