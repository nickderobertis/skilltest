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
    let m = model();
    Command::new(skilltest())
        .arg("run")
        .arg(live_fixtures().join("cases").join(case))
        .args(["--oneharness-bin", &oneharness_bin()])
        .args(["--platform", &platform()])
        .args(["--model", &m])
        .args(["--judge-model", &m])
        .args(["--judge-harness", &platform()])
        .args(["--timeout", "150"])
        .args(["--format", "json"])
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
}
