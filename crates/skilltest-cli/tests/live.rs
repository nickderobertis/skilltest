//! Live end-to-end tests against **real** oneharness + a real harness
//! (claude-code by default). These are the only `#[ignore]` tests in the repo
//! (see `tests/AGENTS.md`): they make real model calls — money, network,
//! non-determinism — so they must never be in the deterministic gate.
//!
//! Run them explicitly, pointing at a built oneharness:
//!
//! ```bash
//! SKILLTEST_ONEHARNESS_BIN=/path/to/oneharness \
//!   cargo test -p skilltest-cli --test live -- --ignored
//! ```
//!
//! Knobs (all optional): `SKILLTEST_LIVE_PLATFORM` (default `claude-code`),
//! `SKILLTEST_LIVE_MODEL` (default `haiku`). The fixtures are intentionally
//! near-deterministic (a skill that always replies "pong") so a real judge has
//! an unambiguous verdict.

use std::path::PathBuf;
use std::process::{Command, Output};

use serde_json::Value;

fn skilltest() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_skilltest"))
}

fn live_fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/live")
}

fn oneharness_bin() -> String {
    std::env::var("SKILLTEST_ONEHARNESS_BIN").unwrap_or_else(|_| "oneharness".into())
}

fn platform() -> String {
    std::env::var("SKILLTEST_LIVE_PLATFORM").unwrap_or_else(|_| "claude-code".into())
}

fn model() -> String {
    std::env::var("SKILLTEST_LIVE_MODEL").unwrap_or_else(|_| "haiku".into())
}

/// Run a live case through the skilltest CLI against real oneharness.
fn run_live(case: &str) -> Output {
    run_live_fmt(case, "json")
}

/// Run a live case through the skilltest CLI against real oneharness, in the
/// given output format (`json` buffered, or `json-stream` for the streaming
/// wire).
fn run_live_fmt(case: &str, format: &str) -> Output {
    live_command(case, format)
        .output()
        .expect("skilltest run executes")
}

/// Like [`run_live`], with `ONEHARNESS_MODE=bypass` so the harness under test
/// is allowed to execute the tool calls the mock/spy cases script (skilltest
/// itself never passes `--mode`; approval policy is oneharness config, exactly
/// as a user would set it).
fn run_live_bypass(case: &str) -> Output {
    live_command(case, "json")
        .env("ONEHARNESS_MODE", "bypass")
        .output()
        .expect("skilltest run executes")
}

fn live_command(case: &str, format: &str) -> Command {
    let m = model();
    let mut cmd = Command::new(skilltest());
    cmd.arg("run")
        .arg(live_fixtures().join("cases").join(case))
        .args(["--oneharness-bin", &oneharness_bin()])
        .args(["--platform", &platform()])
        .args(["--model", &m])
        .args(["--judge-model", &m])
        .args(["--judge-harness", &platform()])
        .args(["--timeout", "150"])
        .args(["--format", format]);
    cmd
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
#[ignore = "live: needs oneharness + a real harness; run with --ignored"]
fn live_respond_and_judge_boolean_and_numeric() {
    let out = run_live("pong.yaml");
    let report = report(&out);
    let run = &report["runs"][0];

    // The skill always says "pong", so both evals should pass against a real judge.
    assert_eq!(
        report["passed"],
        Value::Bool(true),
        "expected pass; report: {report:#}"
    );
    // The assistant actually said pong (deterministic mix-in check on real output).
    let assistant: String = run["transcript"]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["role"] == "assistant")
        .map(|m| m["content"].as_str().unwrap_or("").to_lowercase())
        .collect();
    assert!(assistant.contains("pong"), "assistant said: {assistant}");
    // Both a boolean and a numeric eval ran and passed.
    assert_eq!(run["evals"][0]["detail"]["kind"], "boolean");
    assert_eq!(run["evals"][1]["detail"]["kind"], "numeric");

    // Normalized usage flowed through: every claude-code call reports tokens
    // and cost, so the run *and* the report summary must carry usage with
    // input/output token counts. cost_usd is omitted on subscription auth, so
    // we only require it to be a number when present.
    let usage = &run["usage"];
    assert!(usage.is_object(), "expected per-run usage; got {usage}");
    assert!(
        usage["input_tokens"].as_u64().unwrap_or(0) > 0,
        "expected non-zero input_tokens; got {usage}"
    );
    assert!(
        usage["output_tokens"].as_u64().unwrap_or(0) > 0,
        "expected non-zero output_tokens; got {usage}"
    );
    let summary_usage = &report["summary"]["usage"];
    assert!(
        summary_usage.is_object(),
        "expected summary.usage; got {summary_usage}"
    );
}

#[test]
#[ignore = "live: needs oneharness + a real harness; run with --ignored"]
fn live_multi_turn_drives_simulated_user() {
    let out = run_live("multiturn.yaml");
    let report = report(&out);
    let run = &report["runs"][0];

    assert_eq!(
        report["passed"],
        Value::Bool(true),
        "expected pass; report: {report:#}"
    );
    // Ran to two assistant turns, with a simulated user turn in between.
    assert_eq!(run["turns"], 2, "report: {report:#}");
    let roles: Vec<&str> = run["transcript"]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["role"].as_str().unwrap())
        .collect();
    assert_eq!(roles, ["user", "assistant", "user", "assistant"]);
    // Multi-turn against claude-code goes through --resume, so usage is the
    // sum of two skill turns plus the judge / done_when calls.
    assert!(
        run["usage"].is_object(),
        "expected usage; got {}",
        run["usage"]
    );
}

#[test]
#[ignore = "live: needs oneharness + a real harness; run with --ignored"]
fn live_streaming_emits_ndjson_and_a_terminal_result() {
    // Exercises the *real* `oneharness run --stream` wire (the
    // `OneharnessProvider::run_streaming` path the deterministic gate can only
    // reach via the buffered-replay default): the CLI must emit NDJSON and finish
    // with a `result` line carrying the same kind of report the buffered format
    // returns.
    let out = run_live_fmt("pong.yaml", "json-stream");
    assert!(
        out.status.code() == Some(0) || out.status.code() == Some(1),
        "expected a completed run (exit 0/1), got {:?}; stderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).expect("stdout is utf8");
    let lines: Vec<Value> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("each stream line is one JSON object"))
        .collect();
    assert!(!lines.is_empty(), "stream produced no lines");

    // Any tool-event lines are well-formed; exactly one terminal `result` line
    // closes the stream. (The pong skill uses no tools, so there may be zero
    // event lines — the point is the real `--stream` wire is parsed correctly.)
    let (events, results): (Vec<&Value>, Vec<&Value>) =
        lines.iter().partition(|l| l["type"] == "event");
    for ev in &events {
        assert!(
            ev["event"]["kind"].is_string(),
            "malformed event line: {ev}"
        );
    }
    assert_eq!(results.len(), 1, "exactly one terminal result line");
    let report = &results[0]["report"];
    assert_eq!(report["passed"], Value::Bool(true), "report: {report:#}");

    // The streamed report is a real report: the skill actually said pong.
    let assistant: String = report["runs"][0]["transcript"]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["role"] == "assistant")
        .map(|m| m["content"].as_str().unwrap_or("").to_lowercase())
        .collect();
    assert!(assistant.contains("pong"), "assistant said: {assistant}");
}

// ---------------------------------------------------------------------------
// Tool mocking/spying against the REAL harness — the live drift alarm between
// skilltest's mirrored decision engine (proven in the gate) and the hook-side
// one inside oneharness. The toolrunner skill makes the model run one marked
// shell command; the cases intercept or observe it. `ONEHARNESS_MODE=bypass`
// lets the harness execute tools, exactly as a user configures approval.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "live: needs oneharness + a real harness; run with --ignored"]
fn live_mock_stub_intercepts_a_real_tool_call() {
    let out = run_live_bypass("mock_stub.yaml");
    let report = report(&out);
    let run = &report["runs"][0];
    assert_eq!(
        report["passed"],
        Value::Bool(true),
        "expected pass; report: {report:#}"
    );

    // The spy log recorded the model's REAL attempt with its original input;
    // the stub's rule intercepted it and resolved to the mock's name.
    let records = run["mock_calls"].as_array().expect("mock_calls present");
    let stub = records
        .iter()
        .find(|r| r["action"] == "stub")
        .unwrap_or_else(|| panic!("a stub record; got {records:?}"));
    assert_eq!(stub["mock"], "marker");
    assert!(
        stub["input"]["command"]
            .as_str()
            .unwrap_or_default()
            .contains("MARKER"),
        "the ORIGINAL command is preserved: {stub}"
    );

    // The canned output reached the real model: it reported it back (also
    // judged by the boolean eval; this is the deterministic mix-in check).
    let assistant: String = run["transcript"]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["role"] == "assistant")
        .map(|m| m["content"].as_str().unwrap_or("").to_string())
        .collect();
    assert!(
        assistant.contains("CANNED-BY-SKILLTEST"),
        "assistant said: {assistant}"
    );
}

#[test]
#[ignore = "live: needs oneharness + a real harness; run with --ignored"]
fn live_mock_deny_blocks_a_real_tool_call() {
    let out = run_live_bypass("mock_deny.yaml");
    let report = report(&out);
    let run = &report["runs"][0];
    assert_eq!(
        report["passed"],
        Value::Bool(true),
        "expected pass; report: {report:#}"
    );
    let records = run["mock_calls"].as_array().expect("mock_calls present");
    let denied = records
        .iter()
        .find(|r| r["action"] == "deny")
        .unwrap_or_else(|| panic!("a deny record; got {records:?}"));
    assert_eq!(denied["mock"], "danger");
}

#[test]
#[ignore = "live: needs oneharness + a real harness; run with --ignored"]
fn live_spy_observes_without_intercepting() {
    let out = run_live_bypass("spy_only.yaml");
    let report = report(&out);
    let run = &report["runs"][0];
    assert_eq!(
        report["passed"],
        Value::Bool(true),
        "expected pass; report: {report:#}"
    );
    // Observation only: records exist, none intercepted, and the command's
    // REAL output came back to the model (the boolean eval judged that; the
    // records prove the channel never altered anything).
    let records = run["mock_calls"].as_array().expect("mock_calls present");
    assert!(!records.is_empty(), "the spy recorded the call");
    assert!(
        records.iter().all(|r| r["action"] == "allow"),
        "a spy never intercepts: {records:?}"
    );
}
