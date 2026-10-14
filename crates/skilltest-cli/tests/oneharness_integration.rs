//! Hermetic integration tests against the **real** `oneharness` binary — no
//! harness install, no credentials, no model. The `fake-claude.sh` shim
//! (`tests/fixtures/oneharness/`) stands in for the claude-code CLI via
//! `ONEHARNESS_BIN_CLAUDE_CODE` and — unlike any unit fake — **executes the
//! PreToolUse hook oneharness installs ephemerally**, so these prove the whole
//! mock/spy pipeline with the real seam in the middle:
//!
//!   skilltest compiles `mocks:` → temp rules + `--mock-rules`/`--spy-file` →
//!   oneharness's per-run `--settings` delivery → the real `oneharness mock`
//!   responder matches and renders verdicts → the shim applies them (executing
//!   the substituted printf for a stub, so canned output round-trips) → the
//!   real spy JSONL → skilltest parses it into `mock_calls` → deterministic
//!   `called`/`not_called` evals.
//!
//! Deterministic, but ignored like the live suite because they need the
//! `oneharness` binary on PATH (`just install-oneharness`); run them with
//! `just test-oneharness`. CI's e2e workflows run this before the live phases
//! — it is the drift alarm between skilltest's mirrored decision engine
//! (`mock::decide`, proven in the gate) and oneharness's hook-side one.
//!
// llmlint: ignore-file[shell_test_tiers_stay_split] This file is the host-tool tier, and `tests/AGENTS.md` fixes the split: `just test-oneharness` and the `e2e-*` workflows are its only entry points, and `just check` never reaches it.

use std::path::PathBuf;
use std::process::{Command, Output};

use serde_json::Value;
use skilltest_core::supports_resume;

fn skilltest() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_skilltest"))
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/oneharness")
}

fn shim() -> PathBuf {
    fixtures().join("fake-claude.sh")
}

/// The oneharness binary under test: `$SKILLTEST_ONEHARNESS_BIN`, else
/// `oneharness` on PATH. It reaches a shell in the history replay, so it is
/// held to [`assert_shell_safe`] here, where it enters the suite.
fn oneharness_bin() -> String {
    let bin = std::env::var("SKILLTEST_ONEHARNESS_BIN").unwrap_or_else(|_| "oneharness".into());
    assert_shell_safe("SKILLTEST_ONEHARNESS_BIN", &bin);
    bin
}

/// Refuse a value that a shell would read as more than one plain word: only
/// ASCII letters, digits and `/._+-` pass.
fn assert_shell_safe(what: &str, value: &str) {
    assert!(
        !value.is_empty()
            && value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "/._+-".contains(c)),
        "{what} must be a plain path (ASCII letters, digits, `/._+-`) to be replayed \
         through a shell; got `{value}` — point it at a path without spaces or shell syntax"
    );
}

/// Run a case through the built CLI against real oneharness + the shim.
fn run_case(case: &str, extra: &[&str]) -> Output {
    run_platform(case, "claude-code", extra)
}

fn run_platform(case: &str, platform: &str, extra: &[&str]) -> Output {
    run_platform_with_history_dir(case, platform, extra, None)
}

/// As [`run_platform`], with an optional `SKILLTEST_HISTORY_DIR` so a test can
/// own the history store it asserts on instead of writing to the developer's.
fn run_platform_with_history_dir(
    case: &str,
    platform: &str,
    extra: &[&str],
    history_dir: Option<&std::path::Path>,
) -> Output {
    let mut cmd = Command::new(skilltest());
    cmd.arg("run")
        .arg(fixtures().join("cases").join(case))
        .args(["--oneharness-bin", &oneharness_bin()])
        .args(["--platform", platform])
        .args(["--model", "fake-model"])
        .args(["--judge-harness", "claude-code"])
        .args(["--judge-model", "fake-model"])
        .args(["--timeout", "60"])
        .args(extra)
        // The shim stands in for the claude CLI; oneharness resolves the
        // harness binary from this env override exactly like a user's.
        .env("ONEHARNESS_BIN_CLAUDE_CODE", shim());
    if let Some(dir) = history_dir {
        cmd.env("SKILLTEST_HISTORY_DIR", dir);
    }
    cmd.output().expect("skilltest run executes")
}

/// A private scratch directory removed when the test drops it.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!(
            "skilltest-ohit-{tag}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn report(output: &Output) -> Value {
    assert!(
        output.status.code() == Some(0) || output.status.code() == Some(1),
        "expected a completed run (exit 0/1), got {:?}; stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("stdout is a JSON report")
}

#[test]
#[ignore = "needs oneharness on PATH (just install-oneharness); run via just test-oneharness"]
fn stub_and_spy_round_trip_through_real_oneharness() {
    let out = run_case("mock_stub.yaml", &["--format", "json"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report = report(&out);
    assert_eq!(report["passed"], Value::Bool(true), "report: {report:#}");
    let run = &report["runs"][0];

    // The real spy JSONL came back as records: the stub kept the ORIGINAL
    // input and resolved to its mock's name; the unmocked status call is an
    // `allow`; the rm -rf call (no deny rule in this case) is an allow too.
    let records = run["mock_calls"].as_array().expect("mock_calls present");
    assert_eq!(records.len(), 3, "records: {records:?}");
    assert_eq!(records[0]["action"], "stub");
    assert_eq!(records[0]["mock"], "push");
    assert_eq!(records[0]["input"]["command"], "git push origin main");
    assert_eq!(records[1]["action"], "allow");

    // Post-rewrite reality in the transcript events: the real responder
    // rendered the printf stub, the shim executed it, and the tool_result
    // carries the canned output.
    let events = run["transcript"]["messages"][1]["events"]
        .as_array()
        .expect("assistant turn carries events");
    let stub_call = events[0]["input"]["command"].as_str().unwrap();
    assert!(stub_call.starts_with("printf"), "post-rewrite: {stub_call}");
    let stub_result = events
        .iter()
        .find(|e| e["kind"] == "tool_result")
        .expect("a tool_result event");
    assert_eq!(
        stub_result["output"].as_str().unwrap().trim(),
        "Everything up-to-date"
    );

    // The canned output surfaced in the reply text (the shim appends tool
    // results the way a model relays them).
    let content = run["transcript"]["messages"][1]["content"]
        .as_str()
        .unwrap();
    assert!(content.contains("Everything up-to-date"), "{content}");

    // The deterministic evals scored from the records; the boolean eval went
    // through the judge path — which must never see the mock settings (the
    // shim would reply JUDGE_SAW_SETTINGS) and returns its fixed verdict.
    let evals = run["evals"].as_array().unwrap();
    assert_eq!(evals[0]["detail"]["kind"], "calls");
    assert_eq!(evals[3]["detail"]["kind"], "boolean");
    assert_eq!(evals[3]["reason"], "fake-claude judge");
    assert!(
        !out_text(&out).contains("JUDGE_SAW_SETTINGS"),
        "mocks must never be delivered to the judge"
    );

    // Real oneharness usage flowed through the shim's result document.
    assert!(run["usage"]["input_tokens"].as_u64().unwrap_or(0) > 0);
}

fn out_text(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
#[ignore = "needs oneharness on PATH (just install-oneharness); run via just test-oneharness"]
fn deny_violation_fails_not_called_through_real_oneharness() {
    let out = run_case("mock_violation.yaml", &["--format", "json"]);
    assert_eq!(out.status.code(), Some(1), "a violated expectation exits 1");
    let report = report(&out);
    let eval = &report["runs"][0]["evals"][0];
    assert_eq!(eval["passed"], Value::Bool(false));
    assert_eq!(eval["detail"]["kind"], "calls");
    assert_eq!(eval["detail"]["negated"], true);
    let reason = eval["reason"].as_str().unwrap();
    assert!(
        reason.contains("rm -rf") && reason.contains("[deny]"),
        "reason: {reason}"
    );
    // The deny's message reached the "model": the real responder rendered the
    // claude-nested deny verdict and the shim surfaced its reason.
    let content = report["runs"][0]["transcript"]["messages"][1]["content"]
        .as_str()
        .unwrap();
    assert!(
        content.contains("destructive commands are blocked"),
        "{content}"
    );
}

#[test]
#[ignore = "needs oneharness on PATH (just install-oneharness); run via just test-oneharness"]
fn rewrite_substitutes_input_through_real_oneharness() {
    let out = run_case("mock_rewrite.yaml", &["--format", "json"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report = report(&out);
    let run = &report["runs"][0];
    // The record keeps the original; the matched rule used an `input`
    // per-field predicate (compiled to oneharness's `match.input`).
    let record = run["mock_calls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["action"] == "rewrite")
        .expect("a rewrite record");
    assert_eq!(record["mock"], "redirect");
    assert_eq!(record["input"]["command"], "git status");
    // Post-rewrite reality: the substituted command ran in the shim.
    let content = run["transcript"]["messages"][1]["content"]
        .as_str()
        .unwrap();
    assert!(content.contains("REDIRECTED-OUTPUT"), "{content}");
}

#[test]
#[ignore = "needs oneharness on PATH (just install-oneharness); run via just test-oneharness"]
fn stub_sequence_answers_in_order_through_real_oneharness() {
    // Two turns of two calls each: the real hook rewrites every call to the
    // compiled counter command, the shim executes it, and the run's count
    // spans the conversation (turn 2 resumes the session).
    let out = run_case(
        "../../sequence/mock_stub_sequence.yaml",
        &["--format", "json"],
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report = report(&out);
    let run = &report["runs"][0];
    assert_eq!(run["passed"], Value::Bool(true), "run: {run:#}");
    let records = run["mock_calls"].as_array().expect("mock_calls present");
    assert_eq!(records.len(), 4, "records: {records:?}");
    for record in records {
        // The hook logged a rewrite; the report shows the declared stub with
        // the original input, one record per intercepted call.
        assert_eq!(record["action"], "stub");
        assert_eq!(record["mock"], "status");
        assert_eq!(record["input"]["command"], "jobctl status build-42");
    }
    let outputs: Vec<String> = run["transcript"]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["role"] == "assistant")
        .flat_map(|m| m["events"].as_array().cloned().unwrap_or_default())
        .filter(|e| e["kind"] == "tool_result")
        .map(|e| e["output"].as_str().unwrap_or_default().trim().to_string())
        .collect();
    assert_eq!(
        outputs,
        [
            "build-42: queued",
            "build-42: failed (retrying)",
            "build-42: passed",
            "build-42: passed",
        ]
    );
}

/// One PreToolUse event through the real `oneharness mock claude-code` hook
/// with `rules`, then — as claude-code does — the substituted command through
/// the shell. Returns `(stdout, exit code)` of the command that ran.
fn hook_then_shell(rules: &std::path::Path, spy: &std::path::Path) -> (String, i32) {
    use std::io::Write;
    let mut hook = Command::new(oneharness_bin())
        .args(["mock", "claude-code", "--rules"])
        .arg(rules)
        .arg("--spy-file")
        .arg(spy)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("oneharness mock runs");
    hook.stdin
        .take()
        .unwrap()
        .write_all(br#"{"tool_name":"Bash","tool_input":{"command":"jobctl status build-42"}}"#)
        .unwrap();
    let verdict: Value =
        serde_json::from_slice(&hook.wait_with_output().unwrap().stdout).expect("a verdict");
    let command = verdict["hookSpecificOutput"]["updatedInput"]["command"]
        .as_str()
        .expect("the hook rewrote the command")
        .to_string();
    let ran = Command::new("sh").args(["-c", &command]).output().unwrap();
    (
        String::from_utf8_lossy(&ran.stdout).trim().to_string(),
        ran.status.code().unwrap_or(-1),
    )
}

#[test]
#[ignore = "needs oneharness on PATH (just install-oneharness); run via just test-oneharness"]
fn concurrent_runs_count_their_own_stub_sequences_through_the_real_hook() {
    // Two runs alive at once (two mock sets, as the runner builds per run),
    // their calls interleaved through the real hook: each counts from its own
    // first response, an item's exit code applies to that call alone, and the
    // last response repeats.
    let case =
        skilltest_core::TestCase::load(&fixtures().join("../sequence/mock_stub_sequence.yaml"))
            .expect("the sequence case loads");
    let scratch = Scratch::new("stubseq");
    let runs: Vec<_> = ["a", "b"]
        .iter()
        .map(|tag| {
            let set = skilltest_core::MockSet::build(&[], &case.mocks, false).unwrap();
            let rules = scratch.0.join(format!("{tag}.rules.json"));
            std::fs::write(&rules, set.rules().unwrap().to_string()).unwrap();
            (set, rules, scratch.0.join(format!("{tag}.spy.jsonl")))
        })
        .collect();
    let (a, b) = (&runs[0], &runs[1]);
    let queued = ("build-42: queued".to_string(), 0);
    let failed = ("build-42: failed (retrying)".to_string(), 1);
    let passed = ("build-42: passed".to_string(), 0);
    assert_eq!(hook_then_shell(&a.1, &a.2), queued);
    assert_eq!(hook_then_shell(&b.1, &b.2), queued);
    assert_eq!(hook_then_shell(&a.1, &a.2), failed);
    assert_eq!(hook_then_shell(&a.1, &a.2), passed);
    assert_eq!(hook_then_shell(&b.1, &b.2), failed);
    assert_eq!(hook_then_shell(&a.1, &a.2), passed);
    // The real spy log holds one original-input record per call, resolved to
    // the declared stub.
    let log = std::fs::read_to_string(&a.2).unwrap();
    let records =
        a.0.resolve(skilltest_core::mock::parse_spy_log(&log).unwrap());
    assert_eq!(records.len(), 4);
    assert!(records
        .iter()
        .all(|r| r.action == "stub" && r.mock.as_deref() == Some("status")));
    assert_eq!(
        records[0].input.as_ref().unwrap()["command"],
        "jobctl status build-42"
    );
}

#[test]
#[ignore = "needs oneharness on PATH (just install-oneharness); run via just test-oneharness"]
fn one_item_stub_sequence_is_a_plain_stub_through_the_real_hook() {
    // A one-item list compiles to oneharness's own stub: every call gets that
    // response and its exit code, and the hook itself logs `stub`.
    let decls: Vec<skilltest_core::MockDecl> = serde_saphyr::from_str(
        "- name: status\n  match: { contains: jobctl }\n  stub: [{ output: steady, exit_code: 2 }]\n",
    )
    .unwrap();
    let set = skilltest_core::MockSet::build(&[], &decls, false).unwrap();
    let scratch = Scratch::new("stubone");
    let (rules, spy) = (scratch.0.join("rules.json"), scratch.0.join("spy.jsonl"));
    std::fs::write(&rules, set.rules().unwrap().to_string()).unwrap();
    for _ in 0..2 {
        assert_eq!(hook_then_shell(&rules, &spy), ("steady".to_string(), 2));
    }
    let records =
        skilltest_core::mock::parse_spy_log(&std::fs::read_to_string(&spy).unwrap()).unwrap();
    assert_eq!(records.len(), 2);
    assert!(records.iter().all(|r| r.action == "stub"));
}

#[test]
#[ignore = "needs oneharness on PATH (just install-oneharness); run via just test-oneharness"]
fn spy_flag_records_all_calls_through_real_oneharness() {
    // --spy with no mocks: oneharness installs the pure observer hook
    // (`--spy-file` alone) and every scripted call comes back as `allow`.
    let out = run_case("spy_plain.yaml", &["--spy", "--format", "json"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report = report(&out);
    let records = report["runs"][0]["mock_calls"].as_array().unwrap();
    assert_eq!(records.len(), 3);
    assert!(records.iter().all(|r| r["action"] == "allow"));
    assert!(records.iter().all(|r| r["mock"].is_null()));
    // Nothing was intercepted: the shim's deterministic pass-through outputs.
    let content = report["runs"][0]["transcript"]["messages"][1]["content"]
        .as_str()
        .unwrap();
    assert!(
        !content.contains("[bash]"),
        "no intercepts surfaced: {content}"
    );
}

#[test]
#[ignore = "needs oneharness on PATH (just install-oneharness); run via just test-oneharness"]
fn cli_mocks_file_flows_through_real_oneharness() {
    // Code-level mocks (the SDK path) ride --mocks: the case declares none,
    // the shared stub intercepts, and its name resolves for binding.
    let dir = std::env::temp_dir().join(format!("skilltest-ohit-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mocks = dir.join("mocks.json");
    std::fs::write(
        &mocks,
        r#"[{"name":"push","match":{"tool":"bash","pattern":"git push( --force)?\\b"},"stub":"Everything up-to-date"}]"#,
    )
    .unwrap();
    let out = run_case(
        "spy_plain.yaml",
        &["--mocks", mocks.to_str().unwrap(), "--format", "json"],
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report = report(&out);
    let records = report["runs"][0]["mock_calls"].as_array().unwrap();
    assert_eq!(records[0]["action"], "stub");
    assert_eq!(records[0]["mock"], "push");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
#[ignore = "needs oneharness on PATH (just install-oneharness); run via just test-oneharness"]
fn unsupported_verb_is_refused_loudly_by_real_oneharness() {
    // goose has no rewrite verdict: real oneharness must refuse the stub at
    // startup (its capability registry), skilltest surfaces it as a provider
    // error — never a silent allow-through. No harness binary is ever spawned,
    // so no goose install is needed.
    let out = run_platform("mock_stub.yaml", "goose", &["--format", "json"]);
    assert_eq!(
        out.status.code(),
        Some(3),
        "an inexpressible verb is a provider error; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("cannot express") || stderr.contains("mock action"),
        "the oneharness refusal reaches the user: {stderr}"
    );
}

#[test]
#[ignore = "needs oneharness on PATH (just install-oneharness); run via just test-oneharness"]
fn streaming_with_mocks_through_real_oneharness() {
    // --format json-stream drives `oneharness run --stream --mock-rules …`:
    // NDJSON events flow live and the terminal result still carries the
    // records (the SDKs' streaming mock-binding contract).
    let out = run_case("mock_stub.yaml", &["--format", "json-stream"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).expect("stdout is utf8");
    let lines: Vec<Value> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("one JSON object per line"))
        .collect();
    let (events, results): (Vec<&Value>, Vec<&Value>) =
        lines.iter().partition(|l| l["type"] == "event");
    assert!(!events.is_empty(), "tool events streamed live");
    assert_eq!(results.len(), 1, "exactly one terminal result");
    let report = &results[0]["report"];
    assert_eq!(report["passed"], Value::Bool(true), "report: {report:#}");
    let records = report["runs"][0]["mock_calls"].as_array().unwrap();
    assert_eq!(records[0]["action"], "stub");
    assert_eq!(records[0]["mock"], "push");
}

#[test]
#[ignore = "needs oneharness on PATH (just install-oneharness); run via just test-oneharness"]
fn only_tool_activity_reaches_the_report_and_stream_through_real_oneharness() {
    // The shim writes a `thinking` and a `text` block before each tool call and
    // closes with the reply as text, as a real turn does; oneharness v0.19+
    // reports those as `reasoning`/`message` events numbered among the tool
    // events. Neither the buffered report nor the live stream may surface them.
    let tool = |kind: &Value| kind == "tool_call" || kind == "tool_result";

    let out = run_case("spy_plain.yaml", &["--spy", "--format", "json"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report = report(&out);
    let events: Vec<&Value> = report["runs"][0]["transcript"]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["events"].as_array())
        .flatten()
        .collect();
    let calls = events.iter().filter(|e| e["kind"] == "tool_call").count();
    assert_eq!(calls, 3, "one tool_call per scripted call: {events:#?}");
    assert!(
        events.iter().all(|e| tool(&e["kind"])),
        "only tool activity is surfaced: {events:#?}"
    );
    let indices: Vec<u64> = events
        .iter()
        .map(|e| e["index"].as_u64().unwrap())
        .collect();
    assert!(
        indices.windows(2).all(|w| w[0] < w[1]),
        "kept in order: {indices:?}"
    );
    // The first tool call follows the shim's reasoning and prose, so its index
    // shows oneharness reported (and skilltest dropped) the events before it.
    assert!(
        indices[0] > 0,
        "the binary under test reported no non-tool events, so this proves nothing; \
         run it against the oneharness release scripts/install-oneharness.sh pins: {indices:?}"
    );

    let out = run_case("spy_plain.yaml", &["--spy", "--format", "json-stream"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).expect("stdout is utf8");
    let streamed: Vec<Value> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<Value>(l).expect("one JSON object per line"))
        .filter(|l| l["type"] == "event")
        .collect();
    assert_eq!(
        streamed
            .iter()
            .filter(|l| l["event"]["kind"] == "tool_call")
            .count(),
        3,
        "{streamed:#?}"
    );
    assert!(
        streamed.iter().all(|l| tool(&l["event"]["kind"])),
        "only tool activity is streamed: {streamed:#?}"
    );
}

#[test]
#[ignore = "needs oneharness on PATH (just install-oneharness); run via just test-oneharness"]
fn supports_resume_matches_the_real_registry() {
    // skilltest mirrors oneharness's `supports_resume` column rather than
    // probing it per run, so this is the drift alarm: ask the installed binary
    // what it registers and hold the mirror to it, id by id. A release that
    // adds a harness (or withdraws resume from one) fails here instead of
    // silently routing a resumable harness down the inline-transcript path.
    let out = Command::new(oneharness_bin())
        .args(["list", "--format", "json"])
        .output()
        .expect("oneharness list executes");
    assert!(
        out.status.success(),
        "oneharness list failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let registry: Value = serde_json::from_slice(&out.stdout).expect("list emits JSON");
    let harnesses = registry["harnesses"]
        .as_array()
        .expect("the registry lists harnesses");
    assert!(!harnesses.is_empty(), "registry: {registry:#}");
    for harness in harnesses {
        let id = harness["id"].as_str().expect("every entry has an id");
        // Strictly a bool: a missing or re-typed column is a registry contract
        // change skilltest must be told about, not read as "no resume".
        let registered = harness["supports_resume"]
            .as_bool()
            .unwrap_or_else(|| panic!("{id} has no boolean supports_resume: {harness:#}"));
        assert_eq!(
            supports_resume(id),
            registered,
            "skilltest::supports_resume({id}) disagrees with the installed \
             oneharness registry; update provider::supports_resume"
        );
    }
}

#[test]
#[ignore = "needs oneharness on PATH (just install-oneharness); run via just test-oneharness"]
fn resume_threads_session_id_through_real_oneharness() {
    // Two assistant turns on a resume-capable harness: turn 1 has no session to
    // continue, turn 2 carries the `session_id` the shim echoed. The shim
    // appends ` RESUMED:<id>` whenever oneharness handed it `--resume`, so the
    // transcript itself says which turn continued a session.
    let out = run_case("resume_multiturn.yaml", &["--format", "json"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report = report(&out);
    let messages = report["runs"][0]["transcript"]["messages"]
        .as_array()
        .expect("a transcript");
    let assistant: Vec<&str> = messages
        .iter()
        .filter(|m| m["role"] == "assistant")
        .map(|m| m["content"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(assistant.len(), 2, "messages: {messages:#?}");
    assert!(
        !assistant[0].contains("RESUMED:"),
        "the opening turn has no session to continue: {}",
        assistant[0]
    );
    assert!(
        assistant[1].contains("RESUMED:fake-claude-1"),
        "turn 2 must resume the session the harness reported: {}",
        assistant[1]
    );
}

#[test]
#[ignore = "host-tool tier (the shim this suite drives); run via just test-oneharness"]
fn shim_refuses_resume_without_a_session_id() {
    // The shim stands in for claude-code at oneharness's argv boundary: a bare
    // or empty `--resume` must fail loudly rather than "resume" a blank session,
    // or the resume journey above could pass on an id oneharness never sent.
    for tail in [&["--resume"][..], &["--resume", ""][..]] {
        let out = Command::new(shim())
            .args(["-p", "hello", "--append-system-prompt", "skill"])
            .args(tail)
            .output()
            .expect("the shim runs");
        assert_eq!(out.status.code(), Some(2), "argv tail {tail:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("--resume needs a session id"),
            "stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
#[ignore = "needs oneharness on PATH (just install-oneharness); run via just test-oneharness"]
fn history_recording_round_trips_through_real_oneharness() {
    // Recording is on by default, so a plain run writes a session into the
    // centralized store and the report offers the command that replays it. The
    // proof that the command is real (not just well-formed) is running it.
    let store = Scratch::new("history");
    let out = run_platform_with_history_dir(
        "spy_plain.yaml",
        "claude-code",
        &["--format", "json"],
        Some(&store.0),
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report = report(&out);
    let command = report["runs"][0]["history_command"]
        .as_str()
        .expect("a recorded run offers a history_command");
    // Validate the whole command before running it, so a shell never sees a
    // shape this test did not expect: `<bin> history show <name> --history-dir
    // <store>`, with the session name skilltest derives from the case. The
    // binary and store are held to `assert_shell_safe`; the session name is the
    // part a prefix/suffix check leaves unread, so every character of it is
    // checked against the lowercase-ascii/digit/`-` slug `history_session_name`
    // promises. Together they make the string safe to hand to `sh -c` below.
    let expected_prefix = format!("{} history show skilltest-claude-code-", oneharness_bin());
    assert_shell_safe("the history store path", &store.0.display().to_string());
    let expected_suffix = format!(" --history-dir {}", store.0.display());
    let name_tail = command
        .strip_prefix(&expected_prefix)
        .and_then(|rest| rest.strip_suffix(&expected_suffix))
        .unwrap_or_else(|| {
            panic!("expected `{expected_prefix}<name>{expected_suffix}`, got: {command}")
        });
    assert!(
        !name_tail.is_empty()
            && name_tail
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
        "the session name must be the shell-safe slug skilltest promises, \
         got `{name_tail}` in: {command}"
    );

    // Replay it exactly as a user would read it off the report — running the
    // string verbatim is the point: it is what a reader copies out of the
    // report, so anything less would not prove the offer is real.
    let replay = Command::new("sh")
        .args(["-c", command])
        .output()
        .expect("the history command executes");
    assert!(
        replay.status.success(),
        "`{command}` failed: {}",
        String::from_utf8_lossy(&replay.stderr)
    );
    let recorded = String::from_utf8_lossy(&replay.stdout);
    assert!(
        recorded.contains("claude-code") && recorded.contains("Deployment finished"),
        "the replayed session carries the run: {recorded}"
    );
}

#[test]
#[ignore = "needs oneharness on PATH (just install-oneharness); run via just test-oneharness"]
fn history_off_offers_no_replay_command_through_real_oneharness() {
    // The recovery side of the same seam: with recording disabled no `--history`
    // flag is passed, oneharness echoes no `history_file`, and skilltest must
    // offer no command rather than one that would resolve to nothing.
    let store = Scratch::new("nohistory");
    let config = store.0.join("skilltest.yaml");
    std::fs::write(
        &config,
        format!(
            "provider:\n  kind: oneharness\n  bin: {}\n  judge_harness: claude-code\n  \
             timeout_secs: 60\n  history: false\nplatforms: [claude-code]\nmodels: [fake-model]\n\
             judge_model: fake-model\n",
            oneharness_bin()
        ),
    )
    .expect("config written");
    let out = run_platform_with_history_dir(
        "spy_plain.yaml",
        "claude-code",
        &["--format", "json", "--config", config.to_str().unwrap()],
        Some(&store.0),
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report = report(&out);
    assert!(
        report["runs"][0]["history_command"].is_null(),
        "report: {report:#}"
    );
    assert!(
        std::fs::read_dir(&store.0)
            .expect("store dir")
            .flatten()
            .all(|e| e.file_name() == "skilltest.yaml"),
        "nothing was recorded into {}",
        store.0.display()
    );
}

#[test]
#[ignore = "needs oneharness on PATH (just install-oneharness); run via just test-oneharness"]
fn repo_oneharness_config_is_accepted_by_the_installed_binary() {
    // Every oneharness run started under this tree discovers the root
    // `oneharness.toml`, and oneharness refuses a top-level key it does not
    // know. So the committed file must parse on the targeted line, and the
    // routing must come from it rather than a default or an env override.
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = Command::new(oneharness_bin())
        .args(["config", "--format", "json"])
        .current_dir(&repo_root)
        .output()
        .expect("oneharness config executes");
    assert!(
        out.status.success(),
        "the installed oneharness rejected the repo's oneharness.toml: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let effective: Value = serde_json::from_slice(&out.stdout).expect("config emits JSON");
    assert_eq!(
        effective["run_mode"]["value"], "fallback",
        "config: {effective:#}"
    );
    let source = effective["run_mode"]["source"].as_str().unwrap_or_default();
    assert!(
        source.ends_with("oneharness.toml"),
        "run_mode must come from the committed file, not a default or an env \
         override; source: {source}"
    );
}

#[test]
#[ignore = "needs real oneharness; run via just test-oneharness"]
fn role_configs_deliver_native_and_prompt_schemas_and_retry_settings() {
    let scratch = Scratch::new("roles");
    let log = scratch.0.join("boundary.jsonl");
    let retry = scratch.0.join("retry");
    let judge = scratch.0.join("judge.toml");
    let user = scratch.0.join("user.toml");
    let competing_schema = scratch.0.join("configured-schema.json");
    std::fs::write(&competing_schema,
        serde_json::json!({"type":"object","required":["configured_only"],"properties":{"configured_only":{"type":"string"}},"additionalProperties":false}).to_string()).unwrap();
    std::fs::write(&judge, format!("harnesses = [\"claude-code\"]\nmodel = \"judge-native\"\nmode = \"bypass\"\ntimeout = 17\nschema_max_retries = 1\n[env]\nROLE_MARKER = \"judge\"\nSCHEMA_RETRY_MARKER = {:?}\n", retry.to_string_lossy())).unwrap();
    let judge_config = std::fs::read_to_string(&judge).unwrap();
    std::fs::write(
        &judge,
        judge_config.replace(
            "[env]",
            &format!(
                "schema_file = {:?}\n[env]",
                competing_schema.to_string_lossy()
            ),
        ),
    )
    .unwrap();
    std::fs::write(&user, "harnesses = [\"goose\"]\nmodel = \"user-model\"\nmode = \"bypass\"\ntimeout = 23\n[env]\nROLE_MARKER = \"user\"\nGOOSE_MODEL = \"user-model\"\n").unwrap();
    std::fs::create_dir(scratch.0.join("config")).unwrap();
    let config = scratch.0.join("config/skilltest.json");
    let settings = serde_json::json!({"provider":{"kind":"oneharness","bin":oneharness_bin(),"history":false,"judge":{"config":"judge.toml","args":["--schema-max-retries=1","--no-stream"]},"simulated_user":{"config":"user.toml","args":["--","tail with spaces","--model=literal tail"]}},"models":["responder-model"]});
    std::fs::write(&config, settings.to_string()).unwrap();
    let case = scratch.0.join("case.json");
    std::fs::write(&case, serde_json::json!({"skill":fixtures().join("../skills/greeter"),"input":"hi","user":{"persona":"continue","max_turns":2},"evals":[{"type":"boolean","criterion":"greet"},{"type":"numeric","criterion":"greet","min":0,"max":10,"threshold":7}]}).to_string()).unwrap();
    let invoke = |extra: &[&str]| {
        Command::new(skilltest())
            .current_dir(&scratch.0)
            .arg("--config")
            .arg(&config)
            .args(["run", "--case-json"])
            .arg(&case)
            .args(["--format", "json"])
            .args(extra)
            .env("ONEHARNESS_BIN_CLAUDE_CODE", shim())
            .env("ONEHARNESS_BIN_GOOSE", fixtures().join("fake-goose.sh"))
            .env(
                "ONEHARNESS_BIN_COPILOT",
                scratch.0.join("unavailable-copilot"),
            )
            .env("SKILLTEST_BOUNDARY_LOG", &log)
            .env("ONEHARNESS_STREAM", "true")
            .output()
            .unwrap()
    };
    let output = invoke(&[]);
    let scored = report(&output);
    assert_eq!(scored["passed"], true, "{scored}");
    let observations: Vec<Value> = std::fs::read_to_string(&log)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let judges: Vec<_> = observations
        .iter()
        .filter(|row| row["role"] == "judge")
        .collect();
    assert_eq!(
        judges.len(),
        3,
        "one retry plus boolean and numeric judges: {observations:?}"
    );
    assert!(judges.iter().all(|row| row["model"] == "judge-native"
        && row["mode"] == "bypassPermissions"
        && row["schema"].as_str().is_some_and(|s| !s.is_empty())));
    for row in &judges {
        let schema: Value = serde_json::from_str(row["schema"].as_str().unwrap()).unwrap();
        assert!(schema["properties"]["value"].is_object(), "{schema}");
        assert!(
            schema["properties"].get("configured_only").is_none(),
            "{schema}"
        );
    }
    let users: Vec<_> = observations
        .iter()
        .filter(|row| row["role"] == "user")
        .collect();
    assert_eq!(users.len(), 1);
    assert!(users[0]["argv"]
        .as_array()
        .unwrap()
        .contains(&serde_json::json!("tail with spaces")));
    assert!(users[0]["argv"]
        .as_array()
        .unwrap()
        .contains(&serde_json::json!("--model=literal tail")));
    assert_eq!(users[0]["model"], "user-model");
    assert!(observations
        .iter()
        .filter(|row| row["role"] == "responder")
        .all(|row| row["schema"] == "" && row["model"] == "responder-model"));
    // Switch only the judge to a non-Claude prompt-schema harness, with a failed
    // first fallback candidate. The simulated user keeps its separate config.
    for mode in [vec!["--run-mode", "fallback"], vec!["--run-mode=fallback"]] {
        std::fs::write(&log, "").unwrap();
        let mut role_args = vec![
            "--harness",
            "copilot,goose",
            "--model",
            "prompt-judge",
            "--schema-max-retries",
            "0",
            "--env",
            "ROLE_MARKER=prompt-judge",
        ];
        role_args.extend(mode);
        let assignment = format!("provider.judge={}", serde_json::json!({"args":role_args}));
        let output = invoke(&["--set", &assignment]);
        let scored = report(&output);
        assert_eq!(scored["passed"], true, "{scored}");
        let observations: Vec<Value> = std::fs::read_to_string(&log)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let judges: Vec<_> = observations
            .iter()
            .filter(|row| row["role"] == "prompt-judge")
            .collect();
        assert_eq!(judges.len(), 2);
        assert!(judges.iter().all(|row| row["prompt"]
            .as_str()
            .unwrap()
            .contains("additionalProperties")));
    }
}

#[test]
#[ignore = "needs real oneharness; run via just test-oneharness"]
fn role_inheritance_empty_defaults_named_overrides_and_deadlines() {
    let scratch = Scratch::new("role-precedence");
    let log = scratch.0.join("calls.jsonl");
    std::fs::write(scratch.0.join("oneharness.toml"), "harnesses = [\"claude-code\"]\nmodel = \"ambient-default\"\n[env]\nROLE_MARKER = \"ambient\"\n").unwrap();
    let config = scratch.0.join("config.json");
    let case = scratch.0.join("case.json");
    std::fs::write(&case, serde_json::json!({"skill":fixtures().join("../skills/greeter"),"input":"hello","user":{"persona":"continue","max_turns":2},"evals":[{"type":"boolean","criterion":"greet"}]}).to_string()).unwrap();
    let mut settings = serde_json::json!({"provider":{"kind":"oneharness","bin":oneharness_bin(),"history":false,"judge":{"args":["--harness=claude-code","--model=role-model","--timeout=19","--env","ROLE_MARKER=role"]}},"models":["responder"]});
    let invoke = |settings: &Value, flags: &[&str]| {
        std::fs::write(&config, settings.to_string()).unwrap();
        std::fs::write(&log, "").unwrap();
        Command::new(skilltest())
            .current_dir(&scratch.0)
            .arg("--config")
            .arg(&config)
            .args(["run", "--case-json"])
            .arg(&case)
            .args(["--format", "json"])
            .args(flags)
            .env("ONEHARNESS_BIN_CLAUDE_CODE", shim())
            .env("SKILLTEST_BOUNDARY_LOG", &log)
            .output()
            .unwrap()
    };
    let calls = || {
        std::fs::read_to_string(&log)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>()
    };
    assert_eq!(report(&invoke(&settings, &[]))["passed"], true);
    assert!(calls()
        .iter()
        .any(|call| call["system"] == "" && call["schema"] == "" && call["model"] == "role-model"));
    settings["provider"]["simulated_user"] = serde_json::json!({});
    assert_eq!(report(&invoke(&settings, &[]))["passed"], true);
    assert!(calls().iter().any(|call| call["system"] == ""
        && call["schema"] == ""
        && call["model"] == "ambient-default"));
    settings["provider"]["simulated_user"] = settings["provider"]["judge"].clone();
    assert_eq!(
        report(&invoke(
            &settings,
            &[
                "--judge-harness",
                "claude-code",
                "--judge-model",
                "named",
                "--timeout",
                "21"
            ]
        ))["passed"],
        true
    );
    assert!(calls()
        .iter()
        .filter(|call| call["system"] == "")
        .all(|call| call["model"] == "named"));
    assert_eq!(
        report(&invoke(&settings, &["--judge-model", ""]))["passed"],
        true
    );
    assert!(calls()
        .iter()
        .filter(|call| call["system"] == "")
        .all(|call| call["model"] == "responder"));
    // A role config timeout must govern the actual harness, rather than being
    // replaced by skilltest's legacy 120-second deadline.
    let role = scratch.0.join("deadline.toml");
    std::fs::write(
        &role,
        "harnesses = [\"claude-code\"]\ntimeout = 1\n[env]\nROLE_SLEEP = \"3\"\n",
    )
    .unwrap();
    settings["provider"]["judge"] = serde_json::json!({"config":role});
    settings["provider"]["simulated_user"] = serde_json::json!({});
    let output = invoke(&settings, &[]);
    assert_eq!(output.status.code(), Some(3));
    let error: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(error["kind"], "timeout", "{error}");
}

#[test]
#[ignore = "requires the oneharness fixture host tools"]
fn scripted_boundaries_explain_invalid_inputs() {
    for flag in [
        "--json-schema",
        "--model",
        "--permission-mode",
        "--output-format",
    ] {
        let output = Command::new(shim()).arg(flag).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(&format!("{flag} requires a value")),
            "{stderr}"
        );
        assert!(stderr.contains("rerun"), "{stderr}");
    }
    let output = Command::new(fixtures().join("fake-goose.sh"))
        .arg("-t")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("pass -t <prompt>"));
    let output = Command::new(shim())
        .env("ROLE_SLEEP", "invalid-duration")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("invalid ROLE_SLEEP 'invalid-duration'"),
        "{stderr}"
    );
    assert!(stderr.contains("set a valid sleep duration"), "{stderr}");
}

/// Run the live codex mock phase (`e2e_mock_phase` in `scripts/e2e-lib.sh`,
/// exactly as `scripts/e2e-harness.sh codex` does) with only its paid
/// boundaries doubled: `fake-codex.sh` plays the codex CLI in `scenario`
/// (executing the hook oneharness installs), and `fake-claude.sh` is the judge,
/// which answers `judge_verdict` to every criterion. Everything between — the built
/// `skilltest`, the real `oneharness` and its `mock` hook, the phase's own
/// assertions — is the real thing. Returns the phase's exit code and output.
fn codex_mock_phase(scenario: &str, judge_verdict: &str) -> (Option<i32>, String) {
    let scratch = Scratch::new(&format!("codex-phase-{scenario}"));
    // The phase calls `oneharness` by name, so put the directory of the binary
    // under test first on PATH.
    let oneharness = PathBuf::from(oneharness_bin());
    let mut path = std::env::var_os("PATH").unwrap_or_default();
    if let Some(dir) = oneharness.parent().filter(|d| !d.as_os_str().is_empty()) {
        assert_eq!(
            oneharness.file_name().and_then(|n| n.to_str()),
            Some("oneharness"),
            "the mock phase calls `oneharness` by name; point SKILLTEST_ONEHARNESS_BIN at a \
             binary named `oneharness`"
        );
        let mut dirs = vec![dir.to_path_buf()];
        dirs.extend(std::env::split_paths(&path));
        path = std::env::join_paths(dirs).expect("PATH entries join");
    }
    let lib = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/e2e-lib.sh");
    let workspace = scratch.0.join("ws");
    std::fs::create_dir_all(&workspace).unwrap();
    let out = Command::new("bash")
        .arg("-c")
        .arg(r#"source "$1" && e2e_harness_config codex && e2e_mock_phase codex"#)
        .arg("codex-mock-phase")
        .arg(lib)
        // oneharness installs codex's hook into the project it runs in.
        .current_dir(&workspace)
        .env("PATH", path)
        .env("SKILLTEST_BIN", skilltest())
        .env("SKILLTEST_HISTORY_DIR", scratch.0.join("history"))
        .env("ONEHARNESS_BIN_CODEX", fixtures().join("fake-codex.sh"))
        .env("ONEHARNESS_BIN_CLAUDE_CODE", shim())
        .env("FAKE_CODEX_SCENARIO", scenario)
        .env("FAKE_CLAUDE_VERDICT", judge_verdict)
        .output()
        .expect("bash runs the mock phase");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.code(), text)
}

#[test]
#[ignore = "needs oneharness on PATH (just install-oneharness); run via just test-oneharness"]
fn codex_mock_phase_passes_when_the_stub_output_reaches_the_reply() {
    let (code, text) = codex_mock_phase("relay", "true");
    assert_eq!(code, Some(0), "{text}");
    assert!(
        text.contains("ok: mock phase passed (stub intercepted the marked call)"),
        "{text}"
    );
}

#[test]
#[ignore = "needs oneharness on PATH (just install-oneharness); run via just test-oneharness"]
fn codex_mock_phase_fails_when_the_stub_never_applied() {
    // The hook never fires, so the real command runs and the reply carries
    // its live output: no stub record, whatever the judge says.
    let (code, text) = codex_mock_phase("no-hook", "true");
    assert_eq!(code, Some(1), "{text}");
    assert!(
        text.contains("FAIL: no \"stub\" record — the hook never intercepted the marked call"),
        "{text}"
    );
}

#[test]
#[ignore = "needs oneharness on PATH (just install-oneharness); run via just test-oneharness"]
fn codex_mock_phase_fails_when_the_canned_output_never_reaches_the_reply() {
    // The stub applied, the judge says yes, but the reply never reports the
    // canned output: the phase's reply check, not the judge, decides.
    let (code, text) = codex_mock_phase("ignore", "true");
    assert_eq!(code, Some(1), "{text}");
    assert!(
        text.contains("FAIL: the reply never reports the stub's output CANNED-BY-SKILLTEST"),
        "{text}"
    );
}

#[test]
#[ignore = "needs oneharness on PATH (just install-oneharness); run via just test-oneharness"]
fn codex_mock_phase_fails_when_the_reply_reports_the_live_output() {
    let (code, text) = codex_mock_phase("guess", "true");
    assert_eq!(code, Some(1), "{text}");
    assert!(
        text.contains("FAIL: the reply reports the live output MARKER-LIVE"),
        "{text}"
    );
}

#[test]
#[ignore = "needs oneharness on PATH (just install-oneharness); run via just test-oneharness"]
fn codex_mock_phase_fails_when_the_judge_fails_a_correct_reply() {
    // Interception and reply are right, but a failing eval still fails the run.
    let (code, text) = codex_mock_phase("relay", "false");
    assert_eq!(code, Some(1), "{text}");
    assert!(
        text.contains("\"label\": \"canned-output-reported\""),
        "{text}"
    );
    assert!(
        text.contains("FAIL: the live mock run did not pass; the failing eval's reason is above"),
        "{text}"
    );
}

#[test]
#[ignore = "host-tool tier (bash + jq, beside the oneharness suite); run via just test-oneharness"]
fn mock_assertions_refuse_a_report_whose_reply_cannot_be_read() {
    // A report with the stub record but a non-text assistant message: the
    // reply check stops with a next action instead of reading it as empty.
    let scratch = Scratch::new("unreadable-reply");
    let report = scratch.0.join("report.json");
    std::fs::write(
        &report,
        r#"{"passed":true,"runs":[{"mock_calls":[{"action":"stub","input":{"command":"printf MARKER"}}],
            "transcript":{"messages":[{"role":"assistant","content":{"not":"text"}}]}}]}"#,
    )
    .unwrap();
    let lib = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/e2e-lib.sh");
    let out = Command::new("bash")
        .arg("-c")
        .arg(r#"source "$1" && e2e_assert_mock "$2" stub"#)
        .arg("mock-assertions")
        .arg(lib)
        .arg(&report)
        .output()
        .expect("bash runs the mock assertions");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains("FAIL: could not read the assistant reply from"),
        "{stderr}"
    );
}
