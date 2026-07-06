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

use std::path::PathBuf;
use std::process::{Command, Output};

use serde_json::Value;

fn skilltest() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_skilltest"))
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/oneharness")
}

fn shim() -> PathBuf {
    fixtures().join("fake-claude.sh")
}

fn oneharness_bin() -> String {
    std::env::var("SKILLTEST_ONEHARNESS_BIN").unwrap_or_else(|_| "oneharness".into())
}

/// Run a case through the built CLI against real oneharness + the shim.
fn run_case(case: &str, extra: &[&str]) -> Output {
    run_platform(case, "claude-code", extra)
}

fn run_platform(case: &str, platform: &str, extra: &[&str]) -> Output {
    Command::new(skilltest())
        .arg("run")
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
        .env("ONEHARNESS_BIN_CLAUDE_CODE", shim())
        .output()
        .expect("skilltest run executes")
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
