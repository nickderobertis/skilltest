// Invoked only from release-packaging:packaging, after the real builders ran.
import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import {
  chmodSync,
  cpSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { join, resolve } from "node:path";
import YAML from "yaml";
import { evaluate } from "../workflow-graph.mjs";
const root = process.cwd();
if (process.argv.length !== 4 || !process.argv[2] || !process.argv[3])
  throw new Error(
    "usage: release-journey.mjs <existing-builder-scratch-dir> <declared-native-target>",
  );
const work = resolve(process.argv[2]);
const native = process.argv[3];
if (
  !statSync(work).isDirectory() ||
  !statSync(join(work, "contracts", native)).isDirectory() ||
  !statSync(join(work, "native-products")).isDirectory()
)
  throw new Error("builder scratch inputs missing; run release-product-test");
const declared = JSON.parse(
  execFileSync(
    "python3",
    [
      "-c",
      "import json,tomllib;print(json.dumps([p['target'] for p in tomllib.load(open('release-platforms.toml','rb'))['platform']]))",
    ],
    { encoding: "utf8" },
  ),
);
if (!Array.isArray(declared) || !declared.includes(native))
  throw new Error("undeclared native target");
const fixture = join(work, "transport");
rmSync(fixture, { recursive: true, force: true });
mkdirSync(fixture, { recursive: true });
for (const path of [
  "Cargo.toml",
  "package.json",
  "pnpm-workspace.yaml",
  "release-platforms.toml",
  "sdks/python",
  "plugins/pytest",
  "sdks/typescript",
  "plugins/vitest",
  ".github/npm-identity.mjs",
  ".github/validation.mjs",
  ".github/package.json",
  "scripts/verify-release-archive.sh",
  "scripts/stage-and-verify-release-product.py",
  "scripts/check-release-identity.sh",
])
  cpSync(path, join(fixture, path), {
    recursive: true,
    filter: (src) =>
      !src.includes("node_modules") &&
      !src.includes("/dist/") &&
      !src.includes("/_bin/") &&
      !src.includes("/bin/"),
  });
for (const path of ["sdks/typescript/dist", "plugins/vitest/dist"])
  cpSync(path, join(fixture, path), { recursive: true });
const platforms = JSON.parse(
  execFileSync(
    "python3",
    [
      "-c",
      "import json,tomllib;print(json.dumps(tomllib.load(open('release-platforms.toml','rb'))['platform']))",
    ],
    { encoding: "utf8" },
  ),
);
for (const p of platforms)
  cpSync(join(work, "contracts", p.target, "products"), join(fixture, "products", p.target), {
    recursive: true,
  });
rmSync(join(fixture, "products", native), { recursive: true });
cpSync(join(work, "native-products"), join(fixture, "products", native), { recursive: true });
for (const path of ["sdks/typescript", "plugins/vitest"])
  symlinkSync(resolve(path, "node_modules"), join(fixture, path, "node_modules"), "dir");
const version = JSON.parse(readFileSync("sdks/typescript/package.json", "utf8")).version;
const git = (...args) => execFileSync("git", args, { cwd: fixture, encoding: "utf8" }).trim();
git("init", "--quiet");
git("config", "user.email", "packaging@example.invalid");
git("config", "user.name", "Packaging fixture");
git("add", ".");
git("commit", "--quiet", "-m", "staged release");
const head = git("rev-parse", "HEAD");
git("tag", `v${version}`);
const tools = join(work, "transport-tools");
rmSync(tools, { recursive: true, force: true });
mkdirSync(tools);
const real = Object.fromEntries(
  ["git", "uv", "pnpm", "cargo"].map((command) => [
    command,
    execFileSync("bash", ["-c", `command -v ${command}`], { encoding: "utf8" }).trim(),
  ]),
);
const shim = `#!/usr/bin/env python3
import json, os, pathlib, subprocess, sys
command=pathlib.Path(sys.argv[0]).name
args=sys.argv[1:]
log=pathlib.Path(os.environ['TRANSPORT_LOG'])
def record():
    with log.open('a') as f: f.write(json.dumps({'command':command,'args':args})+'\\n')
flags=int(os.environ['PUBLISHED_FLAGS'])
if command=='git' and args[0]=='fetch': record();sys.exit(0)
if command=='gh':
    record()
    if args[:2]==['release','view']:sys.exit(0 if os.environ.get('EXISTING_RELEASE')=='1' else 1)
    if args[:2]==['release','upload']:
        for path in args[2:]:
            if path.startswith('products/'):assert pathlib.Path(path).is_file()
    sys.exit(0)
if command=='curl':
    url=' '.join(args)
    if '/crates.io/' in url:
        live=bool(flags & 1)
        for line in log.read_text().splitlines():
            call=json.loads(line)
            if call['command']=='cargo' and call['args'][-1] in url:live=True
        sys.exit(0 if live else 22)
    sys.exit(0 if flags & 2 else 22)
if command=='npm' and args[0]=='view':sys.exit(0 if flags & 4 else 1)
if command=='npm' and args[0]=='publish':record();sys.exit(0)
if command=='uv' and args[0]=='publish':
    for path in args[1:]:assert pathlib.Path(path).is_file()
    record();sys.exit(0)
if command=='cargo' and args[0]=='publish':record();sys.exit(0)
if command=='pnpm' and args[0]=='publish':
    out=os.environ['TRANSPORT_PACKS']
    result=subprocess.run([os.environ['REAL_PNPM'],'pack','--pack-destination',out],stdout=subprocess.PIPE,stderr=subprocess.PIPE)
    if result.returncode:sys.stderr.buffer.write(result.stdout+result.stderr);sys.exit(result.returncode)
    record();sys.exit(0)
if command=='cargo':os.chdir(os.environ['REAL_REPO_SOURCE'])
os.execv(os.environ['REAL_'+command.upper()],[command,*args])
`;
for (const command of ["git", "gh", "curl", "npm", "pnpm", "uv", "cargo"]) {
  const path = join(tools, command);
  writeFileSync(path, shim);
  chmodSync(path, 0o755);
}
const publish = YAML.parse(readFileSync(".github/workflows/publish.yml", "utf8")).jobs;
const archive = YAML.parse(
  readFileSync(".github/workflows/release.yml", "utf8"),
).jobs.upload.steps.find((s) => s.run).run;
const commands = {
  crates: publish.crates.steps.find((s) => s.run?.includes("publish_crate skilltest-core")).run,
  pypi: publish.pypi.steps.find((s) => s.run?.includes("on_pypi()")).run,
  npm: publish.npm.steps.find((s) => s.run?.includes("published_npm()")).run,
};
const packs = join(work, "transport-packs");
rmSync(packs, { recursive: true, force: true });
mkdirSync(packs);
const env = {
  ...process.env,
  PATH: `${tools}:${process.env.PATH}`,
  REAL_GIT: real.git,
  REAL_UV: real.uv,
  REAL_PNPM: real.pnpm,
  REAL_CARGO: real.cargo,
  REAL_REPO_SOURCE: root,
  TRANSPORT_PACKS: packs,
  HEAD: head,
  TAG: `v${version}`,
  GITHUB_REPOSITORY: "nickderobertis/skilltest",
};
function run(command, flags, log) {
  return spawnSync("bash", ["-c", command], {
    cwd: fixture,
    encoding: "utf8",
    env: { ...env, PUBLISHED_FLAGS: String(flags), TRANSPORT_LOG: log },
    timeout: 120000,
  });
}
for (let flags = 0; flags < 8; flags++) {
  const log = join(work, `publication-${flags}.jsonl`);
  writeFileSync(log, "");
  for (const command of Object.values(commands)) {
    const result = run(command, flags, log);
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
  }
  const calls = readFileSync(log, "utf8")
    .trim()
    .split("\n")
    .filter(Boolean)
    .map((line) => JSON.parse(line));
  assert.equal(calls.filter((c) => c.command === "cargo").length, flags & 1 ? 0 : 2);
  assert.equal(calls.filter((c) => c.command === "uv").length, flags & 2 ? 0 : 2);
  assert.equal(calls.filter((c) => c.command === "npm").length, flags & 4 ? 0 : platforms.length);
  assert.equal(calls.filter((c) => c.command === "pnpm").length, flags & 4 ? 0 : 2);
  if (!(flags & 2)) {
    const wheels = readdirSync(join(fixture, "sdks/python/dist")).filter((n) => n.endsWith(".whl"));
    for (const p of platforms) assert.ok(wheels.some((n) => n.endsWith(`-${p.wheel_tag}.whl`)));
    assert.ok(wheels.some((n) => n.endsWith("-any.whl")));
    assert.ok(readdirSync(join(fixture, "sdks/python/dist")).some((n) => n.endsWith(".tar.gz")));
  }
}
for (const packed of readdirSync(packs)) {
  const manifest = JSON.parse(
    execFileSync("tar", ["-xOf", join(packs, packed), "package/package.json"], {
      encoding: "utf8",
    }),
  );
  for (const pin of Object.values({ ...manifest.dependencies, ...manifest.optionalDependencies }))
    assert.notEqual(pin, "workspace:*");
}
const uploadLog = join(work, "archive-upload.jsonl");
writeFileSync(uploadLog, "");
const archiveIf = YAML.parse(readFileSync(".github/workflows/release.yml", "utf8")).jobs.upload.if;
for (const conclusion of ["success", "failure"]) {
  const decision = evaluate(archiveIf, {
    github: {
      event: {
        workflow_run: {
          event: "push",
          head_branch: `v${version}`,
          head_repository: { full_name: "nickderobertis/skilltest" },
          conclusion,
        },
      },
    },
  });
  assert.equal(typeof decision === "boolean" ? String(decision) : decision.coerceString(), "true");
  const response = run(archive, 0, uploadLog);
  assert.equal(response.status, 0, `${response.stdout}\n${response.stderr}`);
}
let result = run(archive, 0, uploadLog);
assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
const uploads = readFileSync(uploadLog, "utf8")
  .trim()
  .split("\n")
  .map((line) => JSON.parse(line))
  .filter((c) => c.command === "gh" && c.args[1] === "upload")
  .flatMap((c) => c.args.filter((a) => a.startsWith("products/")));
for (const p of platforms) {
  const asset = `skilltest-${p.target}.${p.bin.endsWith(".exe") ? "zip" : "tar.gz"}`;
  assert.ok(uploads.some((f) => f.endsWith(`/${asset}`)));
  assert.ok(uploads.some((f) => f.endsWith(`/${asset}.sha256`)));
}
const existingLog = join(work, "existing-release.jsonl");
writeFileSync(existingLog, "");
result = spawnSync("bash", ["-c", archive], {
  cwd: fixture,
  encoding: "utf8",
  env: { ...env, PUBLISHED_FLAGS: "0", EXISTING_RELEASE: "1", TRANSPORT_LOG: existingLog },
});
assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
const existingCalls = readFileSync(existingLog, "utf8")
  .trim()
  .split("\n")
  .map((line) => JSON.parse(line));
assert.ok(existingCalls.some((c) => c.command === "gh" && c.args[1] === "upload"));
assert.ok(!existingCalls.some((c) => c.command === "gh" && c.args[1] === "create"));
const wheelDir = join(fixture, "products", platforms[0].target);
const wheelFile = join(
  wheelDir,
  readdirSync(wheelDir).find((n) => n.endsWith(".whl")),
);
const wheelBytes = readFileSync(wheelFile);
rmSync(wheelFile);
const wheelLog = join(work, "missing-wheel.jsonl");
writeFileSync(wheelLog, "");
result = run(commands.pypi, 0, wheelLog);
assert.notEqual(result.status, 0);
assert.match(result.stderr, /missing staged platform wheel/);
assert.equal(readFileSync(wheelLog, "utf8"), "");
writeFileSync(wheelFile, wheelBytes);
for (const [mutation, message] of [
  ["malformed", /BadZipFile/],
  ["duplicate", /duplicate wheel member/],
  ["metadata", /invalid wheel metadata/],
  ["identity", /invalid staged wheel identity/],
  ["payload", /wheel binary differs/],
  ["provider", /wheel contains fake provider/],
]) {
  const changed = spawnSync(
    "python3",
    [
      "-c",
      `
import sys, zipfile, io
path, mutation = sys.argv[1:]
if mutation == "malformed":
    open(path, "wb").write(b"invalid zip")
else:
    source = zipfile.ZipFile(path)
    entries = [(n, source.read(n)) for n in source.namelist()]
    source.close()
    with zipfile.ZipFile(path, "w") as out:
        for name, data in entries:
            if mutation == "metadata" and name.endswith(".dist-info/METADATA"):
                continue
            if mutation == "identity" and name.endswith(".dist-info/METADATA"):
                data = b"Name: wrong\\nVersion: 0.0.0\\n"
            if mutation == "payload" and "/_bin/" in name:
                data = b"wrong binary"
            out.writestr(name, data)
        if mutation == "duplicate":
            out.writestr(entries[0][0], entries[0][1])
        if mutation == "provider":
            out.writestr("skilltest_sdk/_bin/skilltest-fake-provider", b"fake")
`,
      wheelFile,
      mutation,
    ],
    { encoding: "utf8" },
  );
  assert.equal(changed.status, 0, changed.stderr);
  writeFileSync(wheelLog, "");
  result = run(commands.pypi, 0, wheelLog);
  assert.notEqual(result.status, 0, mutation);
  assert.match(result.stderr, message);
  assert.equal(readFileSync(wheelLog, "utf8"), "");
  writeFileSync(wheelFile, wheelBytes);
}
const extraWheel = join(wheelDir, "unexpected.whl");
writeFileSync(extraWheel, wheelBytes);
writeFileSync(wheelLog, "");
result = run(commands.pypi, 0, wheelLog);
assert.notEqual(result.status, 0);
assert.match(result.stderr, /unexpected staged wheel/);
assert.equal(readFileSync(wheelLog, "utf8"), "");
rmSync(extraWheel);
// Carrier metadata and archive corruption must fail at the actual publication boundary.
const carrierPlatform = platforms[0];
const carrier = join(
  fixture,
  "products",
  carrierPlatform.target,
  readdirSync(join(fixture, "products", carrierPlatform.target)).find((n) => n.endsWith(".tgz")),
);
const savedCarrier = readFileSync(carrier);
execFileSync("python3", [
  "-c",
  `import io,json,sys,tarfile
path=sys.argv[1]
with tarfile.open(path) as t: entries=[(m,t.extractfile(m).read()) for m in t.getmembers()]
with tarfile.open(path,'w:gz') as t:
 for m,data in entries:
  if m.name=='package/package.json':
   doc=json.loads(data);doc['name']='@skill-test/cli-forged';data=json.dumps(doc).encode()
  m.size=len(data);t.addfile(m,io.BytesIO(data))`,
  carrier,
]);
result = run(commands.npm, 0, uploadLog);
assert.notEqual(result.status, 0);
assert.match(result.stderr, /carrier identity differs/);
writeFileSync(carrier, savedCarrier);
const archiveName = `skilltest-${carrierPlatform.target}.${carrierPlatform.bin.endsWith(".exe") ? "zip" : "tar.gz"}`;
const archivePath = join(fixture, "products", carrierPlatform.target, archiveName);
const savedArchive = readFileSync(archivePath);
writeFileSync(archivePath, Buffer.concat([savedArchive, Buffer.from("tampered")]));
result = run(archive, 0, uploadLog);
assert.notEqual(result.status, 0);
assert.match(result.stderr, /checksum mismatch/);
writeFileSync(archivePath, savedArchive);
// A missing staged carrier must fail before any package can be substituted.
const first = platforms[0];
rmSync(join(fixture, "products", first.target), { recursive: true });
result = run(commands.npm, 0, uploadLog);
assert.notEqual(result.status, 0);
assert.match(result.stderr, /missing or ambiguous npm carrier/);
result = spawnSync("bash", ["-c", archive], {
  cwd: fixture,
  encoding: "utf8",
  env: { ...env, HEAD: "0".repeat(40), PUBLISHED_FLAGS: "0", TRANSPORT_LOG: uploadLog },
});
assert.notEqual(result.status, 0);
console.log(
  "release transport: actual publication/upload commands passed with only registry/forge transport doubled",
);
