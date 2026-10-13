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

// llmlint: ignore[shell_test_tiers_stay_split] The task requires these deterministic real subprocess boundaries in the integrated workflow-recording test target; no external-service or model probe runs here.
test("screencomp provisioning exercises real filesystem/extraction with only host identity (uname) and forge transport doubled", (t) => {
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
  // A stale cached binary is replaced by the pinned download, not accepted.
  const cached = join(fixture, ".github/.tools/screencomp");
  writeFileSync(cached, '#!/bin/sh\necho "screencomp 0.4.1"\n');
  result = invoke();
  assert.equal(result.status, 0, result.stderr);
  assert.equal(execFileSync(cached, { encoding: "utf8" }).trim(), "screencomp 0.4.2");
  // Every supported host selects its own pinned asset and checksum entry.
  const urls = join(root, "urls");
  writeFileSync(
    join(tools, "curl"),
    '#!/bin/sh\necho "$2" >> "$URL_LOG"\nexec "$REAL_COPY" "$FORGE_ARCHIVE" "$4"\n',
  );
  for (const [os, machine, target] of [
    ["Linux", "x86_64", "x86_64-unknown-linux-gnu"],
    ["Linux", "aarch64", "aarch64-unknown-linux-gnu"],
    ["Darwin", "x86_64", "x86_64-apple-darwin"],
    ["Darwin", "arm64", "aarch64-apple-darwin"],
  ]) {
    rmSync(join(fixture, ".github/.tools"), { recursive: true, force: true });
    writeFileSync(urls, "");
    writeFileSync(
      join(tools, "uname"),
      `#!/bin/sh\ncase "$1" in -s) echo ${os} ;; -m) echo ${machine} ;; esac\n`,
    );
    writeFileSync(lock, JSON.stringify({ [target]: hash }));
    result = spawnSync(bash, [".github/setup-screencomp.sh"], {
      cwd: fixture,
      encoding: "utf8",
      env: { ...process.env, PATH: tools, FORGE_ARCHIVE: archive, REAL_COPY: copy, URL_LOG: urls },
    });
    assert.equal(result.status, 0, `${target}: ${result.stderr}`);
    assert.equal(
      readFileSync(urls, "utf8").trim(),
      `https://github.com/nickderobertis/screencomp/releases/download/v0.4.2/screencomp-v0.4.2-${target}.tar.gz`,
    );
    // The checksum is read for this host's target: another target's entry does not satisfy it.
    rmSync(join(fixture, ".github/.tools"), { recursive: true, force: true });
    writeFileSync(lock, JSON.stringify({ "other-target": hash }));
    result = invoke();
    assert.notEqual(result.status, 0, target);
    assert.match(result.stderr, /cannot read checksum lock/);
  }
  writeFileSync(
    join(tools, "uname"),
    '#!/bin/sh\ncase "$1" in -s) echo Linux ;; -m) echo x86_64 ;; esac\n',
  );
  writeFileSync(join(tools, "curl"), '#!/bin/sh\nexec "$REAL_COPY" "$FORGE_ARCHIVE" "$4"\n');
  writeFileSync(lock, JSON.stringify({ "x86_64-unknown-linux-gnu": hash }));
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
  const restore = () => {
    rmSync(join(fixture, ".github/.tools"), { recursive: true, force: true });
    writeFileSync(join(src, "screencomp"), '#!/bin/sh\necho "screencomp 0.4.2"\n');
    chmodSync(join(src, "screencomp"), 0o755);
    execFileSync("tar", ["-czf", archive, "-C", src, "screencomp"]);
    writeFileSync(
      lock,
      JSON.stringify({
        "x86_64-unknown-linux-gnu": createHash("sha256")
          .update(readFileSync(archive))
          .digest("hex"),
      }),
    );
    writeFileSync(join(tools, "curl"), '#!/bin/sh\nexec "$REAL_COPY" "$FORGE_ARCHIVE" "$4"\n');
    chmodSync(join(tools, "curl"), 0o755);
  };
  restore();
  writeFileSync(join(tools, "uname"), "#!/bin/sh\necho UnsupportedHost\n");
  result = invoke();
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /unsupported host/);
  writeFileSync(
    join(tools, "uname"),
    '#!/bin/sh\ncase "$1" in -s) echo Linux ;; -m) echo x86_64 ;; esac\n',
  );
  restore();
  writeFileSync(join(fixture, ".github/.tools"), "blocking file");
  result = invoke();
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /cannot create/);
  restore();
  writeFileSync(lock, "{");
  result = invoke();
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /cannot read checksum lock/);
  restore();
  writeFileSync(lock, JSON.stringify({ "x86_64-unknown-linux-gnu": "invalid" }));
  result = invoke();
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /missing checksum/);
  restore();
  writeFileSync(join(tools, "curl"), "#!/bin/sh\nexit 22\n");
  result = invoke();
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /download failed/);
  restore();
  writeFileSync(archive, "corrupt gzip");
  writeFileSync(
    lock,
    JSON.stringify({
      "x86_64-unknown-linux-gnu": createHash("sha256").update(readFileSync(archive)).digest("hex"),
    }),
  );
  result = invoke();
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /cannot extract/);
  restore();
  // Use real permission failures rather than replacing hashing or cleanup logic.
  if (process.getuid?.() !== 0) {
    const chmod = execFileSync("bash", ["-c", "command -v chmod"], { encoding: "utf8" }).trim();
    writeFileSync(
      join(tools, "curl"),
      `#!/bin/sh\n"$REAL_COPY" "$FORGE_ARCHIVE" "$4"\n${chmod} 000 "$4"\n`,
    );
    result = invoke();
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /cannot hash download/);
    chmodSync(
      join(fixture, ".github/.tools", "screencomp-v0.4.2-x86_64-unknown-linux-gnu.tar.gz"),
      0o644,
    );
    restore();
    execFileSync("python3", [
      "-c",
      `import io,sys,tarfile
with tarfile.open(sys.argv[1],'w:gz') as t:
 root=tarfile.TarInfo('.');root.type=tarfile.DIRTYPE;root.mode=0o555;t.addfile(root)
 data=b'#!/bin/sh\\necho "screencomp 0.4.2"\\n';bin=tarfile.TarInfo('screencomp');bin.mode=0o755;bin.size=len(data);t.addfile(bin,io.BytesIO(data))`,
      archive,
    ]);
    writeFileSync(
      lock,
      JSON.stringify({
        "x86_64-unknown-linux-gnu": createHash("sha256")
          .update(readFileSync(archive))
          .digest("hex"),
      }),
    );
    result = invoke();
    chmodSync(join(fixture, ".github/.tools"), 0o755);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /cannot remove download/);
  }
});

// llmlint: ignore[shell_test_tiers_stay_split] The task requires these deterministic real subprocess boundaries in the integrated workflow-recording test target; no external-service or model probe runs here.
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
  const actualPayload = event();
  actualPayload.event_name = undefined;
  writeFileSync(file, JSON.stringify(actualPayload));
  result = spawnSync(process.execPath, args, {
    cwd: h.dir,
    encoding: "utf8",
    env: { ...process.env, GITHUB_EVENT_NAME: "pull_request" },
  });
  assert.equal(JSON.parse(result.stdout).isReleaseOnly, true);
  for (const suffix of [["--head", h.release], ["--unexpected", "value"], ["--head"]]) {
    result = spawnSync(process.execPath, [...args, ...suffix], { cwd: h.dir, encoding: "utf8" });
    assert.notEqual(result.status, 0);
  }
});

// llmlint: ignore[shell_test_tiers_stay_split] The task requires these deterministic real subprocess boundaries in the integrated workflow-recording test target; no external-service or model probe runs here.
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
  // A baseline whose bundle smoke still ran an Apple Silicon lane: the table must
  // count it from that baseline and this tree's graph, not restate fixed text.
  const workflows = join(h.dir, ".github", "workflows");
  cpSync(".github/workflows", workflows, { recursive: true });
  const smoke = join(workflows, "bundle-smoke.yml");
  const lane = "          - { target: aarch64-unknown-linux-gnu, os: ubuntu-24.04-arm }\n";
  assert.ok(readFileSync(smoke, "utf8").includes(lane));
  writeFileSync(
    smoke,
    readFileSync(smoke, "utf8").replace(
      lane,
      `${lane}          - { target: aarch64-apple-darwin, os: macos-14 }\n`,
    ),
  );
  const git = (...args) => execFileSync("git", args, { cwd: h.dir, encoding: "utf8" }).trim();
  git("add", ".github");
  git("commit", "--quiet", "-m", "baseline workflows");
  const baseline = git("rev-parse", "HEAD");
  const result = spawnSync(
    process.execPath,
    [".github/scheduling-description.mjs", baseline, observed],
    { encoding: "utf8", env: { ...process.env, GIT_DIR: join(h.dir, ".git") } },
  );
  assert.equal(result.status, 0, result.stderr);
  const rows = result.stdout.split("\n").filter((line) => /^\| [\w-]+\.yml \|/.test(line));
  assert.deepEqual(rows, [
    "| bundle-smoke.yml | 1; push (main); pull_request | 0; push (main); pull_request |",
    "| ci.yml | 1; push (main); pull_request | 1; push (main); pull_request |",
    "| publish.yml | 1; push (v*); workflow_dispatch | 1; push (v*); workflow_dispatch |",
  ]);
  assert.match(
    result.stdout,
    /^\| aarch64-apple-darwin \| skilltest-aarch64-apple-darwin\.tar\.gz \+ sha256 \| macosx_11_0_arm64 \| @skill-test\/cli-darwin-arm64 \|$/m,
  );
  assert.doesNotMatch(result.stdout, /\| x86_64-apple-darwin/);
  assert.match(result.stdout, /Intel macOS artifacts are discontinued from the next release/);
  assert.match(result.stdout, /fixture-head/);
  assert.match(result.stdout, /queued\/pending/);
  assert.match(result.stdout, /not passing-test claims/);
});
