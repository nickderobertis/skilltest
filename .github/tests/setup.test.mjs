import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  chmodSync,
  cpSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { join, resolve } from "node:path";
import test from "node:test";
import { event, history } from "./history.mjs";

test("screencomp provisioning exercises real filesystem/extraction with only forge transport doubled", (t) => {
  const root = mkdtempSync(
    join(process.env.ONEPIPELINE_NODE_SCRATCH_DIR ?? "/tmp", "screencomp-install-"),
  );
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const fixture = join(root, "fixture");
  mkdirSync(join(fixture, ".github"), { recursive: true });
  cpSync(".github/setup-screencomp.sh", join(fixture, ".github/setup-screencomp.sh"));
  const tools = join(root, "tools");
  mkdirSync(tools);
  const system = {
    node: process.execPath,
    python3: execFileSync("bash", ["-c", "command -v python3"], { encoding: "utf8" }).trim(),
  };
  for (const name of ["dirname", "mkdir", "tar", "rm", "gzip"])
    system[name] = execFileSync("bash", ["-c", `command -v ${name}`], { encoding: "utf8" }).trim();
  for (const [name, path] of Object.entries(system))
    execFileSync("ln", ["-s", resolve(path), join(tools, name)]);
  writeFileSync(
    join(tools, "uname"),
    '#!/bin/sh\ncase "$1" in -s) echo Linux ;; -m) echo x86_64 ;; esac\n',
  );
  chmodSync(join(tools, "uname"), 0o755);
  writeFileSync(join(tools, "curl"), '#!/bin/sh\nexec "$REAL_COPY" "$FORGE_ARCHIVE" "$4"\n');
  chmodSync(join(tools, "curl"), 0o755);
  const copy = execFileSync("bash", ["-c", "command -v cp"], { encoding: "utf8" }).trim();
  const src = join(root, "src");
  mkdirSync(src);
  writeFileSync(join(src, "screencomp"), '#!/bin/sh\necho "screencomp 0.4.2"\n');
  chmodSync(join(src, "screencomp"), 0o755);
  const archive = join(root, "screencomp.tar.gz");
  execFileSync("tar", ["-czf", archive, "-C", src, "screencomp"]);
  const hash = createHash("sha256").update(readFileSync(archive)).digest("hex");
  const lock = join(fixture, ".github/screencomp-lock.json");
  writeFileSync(lock, JSON.stringify({ "x86_64-unknown-linux-gnu": hash }));
  // bash is invoked by absolute path so the fixture PATH controls only its tools.
  const bash = execFileSync("bash", ["-c", "command -v bash"], { encoding: "utf8" }).trim();
  const invoke = () =>
    spawnSync(bash, [".github/setup-screencomp.sh"], {
      cwd: fixture,
      encoding: "utf8",
      env: { ...process.env, PATH: tools, FORGE_ARCHIVE: archive, REAL_COPY: copy },
    });
  let result = invoke();
  assert.equal(result.status, 0, result.stderr);
  result = invoke();
  assert.equal(result.status, 0, result.stderr);
  rmSync(join(fixture, ".github/.tools"), { recursive: true });
  writeFileSync(lock, JSON.stringify({ "x86_64-unknown-linux-gnu": "0".repeat(64) }));
  result = invoke();
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /checksum changed/);
  writeFileSync(lock, JSON.stringify({ "x86_64-unknown-linux-gnu": hash }));
  writeFileSync(join(tools, "screencomp"), '#!/bin/sh\necho "screencomp 0.4.2"\n');
  chmodSync(join(tools, "screencomp"), 0o755);
  result = invoke();
  assert.equal(result.status, 0, result.stderr);
  rmSync(join(tools, "screencomp"));
  writeFileSync(join(src, "screencomp"), '#!/bin/sh\necho "screencomp 9.9.9"\n');
  execFileSync("tar", ["-czf", archive, "-C", src, "screencomp"]);
  writeFileSync(
    lock,
    JSON.stringify({
      "x86_64-unknown-linux-gnu": createHash("sha256").update(readFileSync(archive)).digest("hex"),
    }),
  );
  rmSync(join(fixture, ".github/.tools"), { recursive: true });
  result = invoke();
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /wrong binary version/);
});

test("classifier emits the actual GitHub outputs and rejects unsupported event/flag inputs", (t) => {
  const h = history(t);
  const output = join(h.dir, ".git", "outputs");
  const file = join(h.dir, ".git", "event");
  writeFileSync(file, JSON.stringify(event()));
  const args = [
    resolve(".github/macos-policy.mjs"),
    "classify",
    "--event",
    file,
    "--base",
    h.base,
    "--head",
    h.release,
  ];
  let result = spawnSync(process.execPath, args, {
    cwd: h.dir,
    encoding: "utf8",
    env: { ...process.env, GITHUB_OUTPUT: output },
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(readFileSync(output, "utf8"), "macosRequired=false\nisReleaseOnly=true\n");
  writeFileSync(file, JSON.stringify(event({ event_name: "unknown-event" })));
  result = spawnSync(process.execPath, args, { cwd: h.dir, encoding: "utf8" });
  assert.equal(JSON.parse(result.stdout).macosRequired, true);
  for (const suffix of [["--head", h.release], ["--unexpected", "value"], ["--head"]]) {
    result = spawnSync(process.execPath, [...args, ...suffix], { cwd: h.dir, encoding: "utf8" });
    assert.notEqual(result.status, 0);
  }
});

test("description reads the actual baseline graph and renders typed hosted observations", (t) => {
  const h = history(t);
  const observed = join(h.dir, ".git", "observed.json");
  writeFileSync(
    observed,
    JSON.stringify({
      head: "fixture-head",
      runs: [
        {
          name: "ci",
          html_url: "https://example.invalid/run",
          status: "queued",
          conclusion: null,
          jobs: [{ name: "check (macos-latest)", status: "queued", conclusion: null }],
        },
      ],
    }),
  );
  const result = spawnSync(
    process.execPath,
    [".github/scheduling-description.mjs", "origin/main", observed],
    { encoding: "utf8" },
  );
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /ci.yml/);
  assert.match(result.stdout, /fixture-head/);
  assert.match(result.stdout, /queued\/pending/);
  assert.match(result.stdout, /not passing-test claims/);
});
