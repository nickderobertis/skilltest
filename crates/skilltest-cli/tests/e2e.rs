//! End-to-end tests that drive the **built** `skilltest` binary the way a user
//! does — as a subprocess, asserting on exit codes and JSON output — against the
//! deterministic `skilltest-fake-provider`. Only the model is faked; everything
//! else (arg parsing, YAML loading, the conversation loop, evals, the JSON
//! contract, exit codes) is the real thing.
//!
//! Exit codes under test (see `skilltest_core::ExitCode`): 0 success, 1 a test
//! case / skill failed, 2 bad input, 3 provider failure.

use std::path::PathBuf;
use std::process::{Command, Output};

use serde_json::Value;

/// Path to the built `skilltest` binary (provided by Cargo for integration tests).
fn skilltest() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_skilltest"))
}

/// Path to the fake provider, which Cargo builds into the same directory.
fn fake_provider() -> PathBuf {
    skilltest()
        .parent()
        .expect("binary has a parent dir")
        .join("skilltest-fake-provider")
}

/// Absolute path to the shared fixtures directory at the repo root.
fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn case(name: &str) -> PathBuf {
    fixtures().join("cases").join(name)
}

/// Run `skilltest run <path> [extra...]` wired to the fake provider.
fn run_case(path: PathBuf, extra: &[&str]) -> Output {
    let mut cmd = Command::new(skilltest());
    cmd.arg("run")
        .arg(path)
        .arg("--provider")
        .arg(fake_provider())
        .args(["--platform", "demo", "--model", "fake"])
        .args(extra);
    cmd.output().expect("skilltest run executes")
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("stdout is valid JSON")
}

#[test]
fn happy_path_single_turn_passes() {
    let out = run_case(case("greet_pass.yaml"), &["--format", "json"]);
    assert!(out.status.success(), "expected exit 0");
    let report = json(&out);
    assert_eq!(report["passed"], Value::Bool(true));
    assert_eq!(report["summary"]["runs"], 1);
    assert_eq!(report["runs"][0]["turns"], 1);
    // Both evals present and passing.
    assert_eq!(report["runs"][0]["evals"].as_array().unwrap().len(), 2);
}

#[test]
fn numeric_eval_passes_above_threshold() {
    let out = run_case(case("greet_numeric.yaml"), &["--format", "json"]);
    assert!(out.status.success());
    let report = json(&out);
    let eval = &report["runs"][0]["evals"][0];
    assert_eq!(eval["passed"], Value::Bool(true));
    assert_eq!(eval["detail"]["kind"], "numeric");
}

#[test]
fn multi_turn_stops_at_done_condition() {
    let out = run_case(case("booking_multiturn.yaml"), &["--format", "json"]);
    assert!(out.status.success());
    let report = json(&out);
    // done_when `turns>=2` ends the conversation at exactly two assistant turns.
    assert_eq!(report["runs"][0]["turns"], 2);
    let roles: Vec<&str> = report["runs"][0]["transcript"]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["role"].as_str().unwrap())
        .collect();
    assert_eq!(roles, ["user", "assistant", "user", "assistant"]);
}

#[test]
fn failing_eval_exits_one_and_reports_failure() {
    let out = run_case(case("greet_fail.yaml"), &["--format", "json"]);
    assert_eq!(out.status.code(), Some(1), "a failing eval exits 1");
    let report = json(&out);
    assert_eq!(report["passed"], Value::Bool(false));
    assert_eq!(report["runs"][0]["evals"][0]["passed"], Value::Bool(false));
}

#[test]
fn running_a_directory_discovers_and_aggregates_every_case() {
    // The cases directory contains the failing case, so the aggregate fails.
    let out = run_case(fixtures().join("cases"), &["--format", "json"]);
    assert_eq!(out.status.code(), Some(1));
    let report = json(&out);
    assert_eq!(report["summary"]["runs"], 4);
    assert!(report["summary"]["failed"].as_u64().unwrap() >= 1);
}

#[test]
fn missing_provider_exits_three() {
    let out = Command::new(skilltest())
        .arg("run")
        .arg(case("greet_pass.yaml"))
        .args(["--provider", "/nonexistent/provider-binary"])
        .args(["--platform", "demo", "--model", "fake"])
        .output()
        .expect("executes");
    assert_eq!(out.status.code(), Some(3), "provider failure exits 3");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("provider"),
        "stderr explains the provider: {stderr}"
    );
}

#[test]
fn malformed_test_case_exits_two() {
    let dir = std::env::temp_dir().join(format!("skilltest-e2e-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let bad = dir.join("bad.yaml");
    std::fs::write(&bad, "skill: ./x\ninput: hi\nbogus_field: 1\nevals: []\n").unwrap();
    let out = run_case(bad, &[]);
    assert_eq!(out.status.code(), Some(2), "bad input exits 2");
}

#[test]
fn validate_accepts_a_good_skill() {
    let out = Command::new(skilltest())
        .arg("validate")
        .arg(fixtures().join("skills/greeter"))
        .output()
        .expect("executes");
    assert!(out.status.success(), "valid skill exits 0");
}

#[test]
fn validate_rejects_an_invalid_skill() {
    let out = Command::new(skilltest())
        .arg("validate")
        .arg(fixtures().join("skills/invalid"))
        .output()
        .expect("executes");
    assert_eq!(out.status.code(), Some(1), "invalid skill exits 1");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("description"),
        "stderr names the missing field: {stderr}"
    );
}

#[test]
fn help_exits_zero() {
    let out = Command::new(skilltest())
        .arg("--help")
        .output()
        .expect("executes");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("run"));
    assert!(stdout.contains("validate"));
}
