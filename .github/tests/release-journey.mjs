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
  "scripts/verify-release-archive.sh",
  "scripts/verify-staged-archives.sh",
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
        if os.environ.get('UPLOAD_FAILS')=='1':sys.exit(1)
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
// Exactly the declared archives and checksums reach the forge, nothing else.
const declaredAssets = platforms.flatMap((p) => {
  const asset = `products/${p.target}/skilltest-${p.target}.${p.bin.endsWith(".exe") ? "zip" : "tar.gz"}`;
  return [asset, `${asset}.sha256`];
});
assert.deepEqual([...new Set(uploads)].sort(), declaredAssets.sort());
// An undeclared asset staged in products stops the upload before any is sent.
const stray = join(fixture, "products", platforms[0].target, "skilltest-undeclared.zip");
writeFileSync(stray, "not a declared release asset");
const strayLog = join(work, "stray-asset.jsonl");
writeFileSync(strayLog, "");
result = run(archive, 0, strayLog);
assert.notEqual(result.status, 0);
assert.match(result.stderr, /skilltest-undeclared\.zip is not a declared release asset/);
assert.ok(!readFileSync(strayLog, "utf8").includes('"upload"'));
rmSync(stray);
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
// A refused forge upload must fail the production upload command, not vanish inside find.
const failedUploadLog = join(work, "failed-upload.jsonl");
writeFileSync(failedUploadLog, "");
result = spawnSync("bash", ["-c", archive], {
  cwd: fixture,
  encoding: "utf8",
  env: { ...env, PUBLISHED_FLAGS: "0", UPLOAD_FAILS: "1", TRANSPORT_LOG: failedUploadLog },
});
assert.notEqual(result.status, 0);
assert.ok(
  readFileSync(failedUploadLog, "utf8")
    .trim()
    .split("\n")
    .map((line) => JSON.parse(line))
    .some((c) => c.command === "gh" && c.args[1] === "upload"),
);
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
  ["malformed", /File is not a zip file/],
  ["duplicate", /duplicate wheel member/],
  ["metadata", /invalid wheel metadata/],
  ["identity", /invalid staged wheel identity/],
  ["payload", /wheel binary differs/],
  ["provider", /wheel contains fake provider/],
  ["tags", /invalid wheel tags/],
  ["module", /wheel module differs from source: skilltest_sdk\/runner\.py/],
  ["extra", /unexpected wheel members: extra \['skilltest_sdk\/injected\.py'\]/],
  ["directory", /wheel holds a directory entry/],
  ["record-row", /invalid wheel RECORD row/],
  ["record-duplicate", /duplicate wheel RECORD row: skilltest_sdk\/runner\.py/],
  ["record-missing", /wheel RECORD does not list exactly its members/],
  ["record-self", /wheel RECORD hashes itself/],
  ["record-hash", /wheel RECORD hash differs: skilltest_sdk\/runner\.py/],
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
            if mutation == "module" and name == "skilltest_sdk/runner.py":
                data = data + b"\\nraise SystemExit('altered')\\n"
            if name.endswith(".dist-info/RECORD") and mutation.startswith("record-"):
                rows = data.decode().splitlines()
                runner = next(r for r in rows if r.startswith("skilltest_sdk/runner.py,"))
                if mutation == "record-row":
                    rows.append("skilltest_sdk/runner.py,sha256=x")
                if mutation == "record-duplicate":
                    rows.append(runner)
                if mutation == "record-missing":
                    rows.remove(runner)
                if mutation == "record-self":
                    rows = [r + "sha256=x" if r.endswith("/RECORD,,") else r for r in rows]
                if mutation == "record-hash":
                    rows[rows.index(runner)] = runner.split(",")[0] + ",sha256=AAAA," + runner.split(",")[2]
                data = ("\\n".join(rows) + "\\n").encode()
            if mutation == "tags" and name.endswith(".dist-info/WHEEL"):
                data = b"Wheel-Version: 1.0\\nRoot-Is-Purelib: true\\nTag: py3-none-any\\n"
            out.writestr(name, data)
        if mutation == "duplicate":
            out.writestr(entries[0][0], entries[0][1])
        if mutation == "directory":
            out.writestr(zipfile.ZipInfo("skilltest_sdk/sub/"), b"")
        if mutation == "extra":
            out.writestr("skilltest_sdk/injected.py", b"print('injected')\\n")
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
// The wheel check reads each wheel's binary from its staged archive, so a
// missing, re-checksummed or widened archive must refuse publication.
const wheelArchive = join(
  wheelDir,
  `skilltest-${platforms[0].target}.${platforms[0].bin.endsWith(".exe") ? "zip" : "tar.gz"}`,
);
const wheelArchiveBytes = readFileSync(wheelArchive);
const wheelChecksum = `${wheelArchive}.sha256`;
const wheelChecksumText = readFileSync(wheelChecksum, "utf8");
for (const [mutation, message] of [
  ["missing", /missing staged archive/],
  ["checksum", /staged archive checksum differs/],
  ["widened", /unexpected archive contents/],
]) {
  if (mutation === "missing") rmSync(wheelArchive);
  if (mutation === "checksum") writeFileSync(wheelChecksum, `${"0".repeat(64)}  x\n`);
  if (mutation === "widened")
    execFileSync("python3", [
      "-c",
      `import hashlib,io,os,sys,tarfile,zipfile
path,checksum=sys.argv[1:]
if path.endswith('.zip'):
 with zipfile.ZipFile(path,'a') as z: z.writestr('extra','x')
else:
 with tarfile.open(path) as t: entries=[(m,t.extractfile(m).read()) for m in t.getmembers()]
 with tarfile.open(path,'w:gz') as t:
  for m,data in entries: t.addfile(m,io.BytesIO(data))
  extra=tarfile.TarInfo('extra');extra.size=1;t.addfile(extra,io.BytesIO(b'x'))
open(checksum,'w').write(hashlib.sha256(open(path,'rb').read()).hexdigest()+'  '+os.path.basename(path)+'\\n')`,
      wheelArchive,
      wheelChecksum,
    ]);
  writeFileSync(wheelLog, "");
  result = run(commands.pypi, 0, wheelLog);
  assert.notEqual(result.status, 0, mutation);
  assert.match(result.stderr, message);
  assert.equal(readFileSync(wheelLog, "utf8"), "");
  writeFileSync(wheelArchive, wheelArchiveBytes);
  writeFileSync(wheelChecksum, wheelChecksumText);
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
const carrierLog = join(work, "corrupt-carrier.jsonl");
writeFileSync(carrierLog, "");
result = run(commands.npm, 0, carrierLog);
assert.notEqual(result.status, 0);
assert.match(result.stderr, /carrier identity differs/);
assert.equal(readFileSync(carrierLog, "utf8"), "");
writeFileSync(carrier, savedCarrier);
// Payload and member corruption must also refuse before any npm publish.
for (const [mutation, message] of [
  ["payload", /npm binary differs/],
  ["member", /unexpected npm contents/],
  ["duplicate", /unexpected npm contents/],
  ["link", /unexpected npm contents/],
  ["scripts", /packed npm manifest differs from source/],
]) {
  execFileSync("python3", [
    "-c",
    `import io,json,sys,tarfile
path,mutation=sys.argv[1:]
with tarfile.open(path) as t: entries=[(m,t.extractfile(m).read()) for m in t.getmembers()]
with tarfile.open(path,'w:gz') as t:
 for m,data in entries:
  if m.name.startswith('package/bin/'):
   if mutation=='payload': data=b'not the archived binary'
   if mutation=='link':
    m.type=tarfile.SYMTYPE;m.linkname='/bin/sh';m.size=0;t.addfile(m);continue
  if mutation=='scripts' and m.name=='package/package.json':
   doc=json.loads(data);doc['scripts']={'postinstall':'node -e 0'};data=json.dumps(doc).encode()
  if mutation=='duplicate' and m.name=='package/package.json': t.addfile(m,io.BytesIO(data))
  m.size=len(data);t.addfile(m,io.BytesIO(data))
 if mutation=='member':
  extra=tarfile.TarInfo('package/postinstall.js');extra.size=1;t.addfile(extra,io.BytesIO(b'x'))`,
    carrier,
    mutation,
  ]);
  writeFileSync(carrierLog, "");
  result = run(commands.npm, 0, carrierLog);
  assert.notEqual(result.status, 0, mutation);
  assert.match(result.stderr, message, mutation);
  assert.equal(readFileSync(carrierLog, "utf8"), "", mutation);
  writeFileSync(carrier, savedCarrier);
}
const extraCarrier = join(fixture, "products", "skill-test-cli-extra.tgz");
writeFileSync(extraCarrier, savedCarrier);
writeFileSync(carrierLog, "");
result = run(commands.npm, 0, carrierLog);
assert.notEqual(result.status, 0);
assert.match(result.stderr, /unexpected staged npm carrier/);
assert.equal(readFileSync(carrierLog, "utf8"), "");
rmSync(extraCarrier);
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
