//! End-to-end coverage of the CLI's error-classification and dispatch paths,
//! driving the **built** `skilltest` binary the way users do — as a subprocess.
//!
//! These complement `e2e.rs` (the happy/representative journeys) by exercising
//! the provider-error hint branches (`report_error`), the explicit `--config`
//! path, the validate JSON/human surfaces, and the API-judge wiring — all
//! offline, against deterministic fake `oneharness` / provider scripts.

#![cfg(unix)]

use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

fn skilltest() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_skilltest"))
}

fn fake_provider() -> PathBuf {
    skilltest()
        .parent()
        .expect("binary has a parent dir")
        .join("skilltest-fake-provider")
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

/// A unique temp dir for one test artifact set.
fn temp_dir(tag: &str) -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "skilltest-clierr-{}-{tag}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Write an executable shell script into `dir` and return its path.
fn script(dir: &std::path::Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(format!("#!/bin/sh\n{body}").as_bytes())
        .unwrap();
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).unwrap();
    path
}

/// A fake `oneharness` that emits a single result with the given `failure_kind`,
/// so the CLI's classified-error hint for that kind is exercised end to end.
fn fake_oneharness(dir: &std::path::Path, failure_kind: &str) -> PathBuf {
    script(
        dir,
        "oneharness",
        &format!(
            "cat >/dev/null\necho '{{\"results\":[{{\"status\":\"error\",\
             \"failure_kind\":\"{failure_kind}\",\"error\":\"simulated {failure_kind}\"}}]}}'\n"
        ),
    )
}

fn run_passing_case(extra: &[&str]) -> Output {
    let mut cmd = Command::new(skilltest());
    cmd.arg("run")
        .arg(fixtures().join("cases/greet_pass.yaml"))
        .arg("--provider")
        .arg(fake_provider())
        .args(["--platform", "demo", "--model", "fake"])
        .args(extra);
    cmd.output().expect("skilltest run executes")
}

#[test]
fn classified_provider_errors_print_their_hint() {
    // Each failure_kind the harness can report maps to a distinct, pointed hint
    // and always exits with the provider-error code (3).
    let cases = [
        ("auth", "credentials"),
        ("rate_limit", "rate-limited"),
        ("model_not_found", "does not recognize this model"),
        ("quota", "quota"),
        ("overloaded", "overloaded"),
        ("timeout", "timed out"),
        ("spawn", "could not start the provider"),
        ("protocol", "violated the protocol"),
        ("some_other_kind", "see provider docs"),
    ];
    for (kind, needle) in cases {
        let dir = temp_dir(kind);
        let oh = fake_oneharness(&dir, kind);
        let out = Command::new(skilltest())
            .arg("run")
            .arg(fixtures().join("cases/greet_pass.yaml"))
            .args(["--oneharness-bin", oh.to_str().unwrap()])
            .args(["--platform", "claude-code", "--model", "sonnet"])
            .output()
            .expect("executes");
        assert_eq!(
            out.status.code(),
            Some(3),
            "{kind} should exit 3, stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains(needle),
            "{kind} hint should mention {needle:?}, got: {stderr}"
        );
    }
}

#[test]
fn json_format_provider_error_emits_structured_error() {
    // On `--format json`, a provider failure emits the structured error on stdout
    // (carrying the classified `kind`/`context`) while the human hint still goes
    // to stderr — so an SDK consumer branches on `kind`, not the message string.
    let dir = temp_dir("json-provider-err");
    let oh = fake_oneharness(&dir, "auth");
    let out = Command::new(skilltest())
        .arg("run")
        .arg(fixtures().join("cases/greet_pass.yaml"))
        .args(["--oneharness-bin", oh.to_str().unwrap()])
        .args(["--platform", "claude-code", "--model", "sonnet"])
        .args(["--format", "json"])
        .output()
        .expect("executes");
    assert_eq!(out.status.code(), Some(3));
    let err: Value = serde_json::from_slice(&out.stdout).expect("stdout is the JSON error");
    assert_eq!(err["code"], "provider");
    assert_eq!(err["kind"], "auth");
    assert_eq!(err["context"], "oneharness:claude-code");
    assert!(
        err["message"]
            .as_str()
            .unwrap()
            .contains("harness run failed"),
        "message: {err}"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("credentials"),
        "human hint still on stderr"
    );
}

#[test]
fn json_format_classifies_a_timeout_status() {
    // oneharness reports a deadline as `status: "timeout"` with no `failure_kind`.
    // skilltest still classifies it, so the JSON error carries `kind: "timeout"`
    // — the case that motivated structured errors (no message-string parsing).
    let dir = temp_dir("json-timeout");
    let oh = script(
        &dir,
        "oneharness",
        "cat >/dev/null\necho '{\"results\":[{\"status\":\"timeout\",\
         \"stderr\":\"deadline exceeded\"}]}'\n",
    );
    let out = Command::new(skilltest())
        .arg("run")
        .arg(fixtures().join("cases/greet_pass.yaml"))
        .args(["--oneharness-bin", oh.to_str().unwrap()])
        .args(["--platform", "claude-code", "--model", "sonnet"])
        .args(["--format", "json"])
        .output()
        .expect("executes");
    assert_eq!(out.status.code(), Some(3));
    let err: Value = serde_json::from_slice(&out.stdout).expect("stdout is the JSON error");
    assert_eq!(err["kind"], "timeout");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("timed out"),
        "timeout hint on stderr"
    );
}

#[test]
fn json_format_usage_error_emits_structured_error_without_kind() {
    // A usage error (exit 2) is emitted in JSON mode too, as `code: "usage"` with
    // no provider `kind`/`context`.
    let dir = temp_dir("json-usage-err");
    let bad = dir.join("bad.yaml");
    std::fs::write(&bad, "input: [unterminated\n").unwrap();
    let out = Command::new(skilltest())
        .arg("run")
        .arg(&bad)
        .arg("--provider")
        .arg(fake_provider())
        .args(["--platform", "demo", "--model", "fake", "--format", "json"])
        .output()
        .expect("executes");
    assert_eq!(out.status.code(), Some(2));
    let err: Value = serde_json::from_slice(&out.stdout).expect("stdout is the JSON error");
    assert_eq!(err["code"], "usage");
    assert!(err["kind"].is_null(), "usage errors carry no kind: {err}");
    assert!(err["context"].is_null());
}

#[test]
fn json_stream_error_emits_a_terminal_error_line() {
    // On `--format json-stream`, a failure is the terminal `{"type":"error",…}`
    // NDJSON line, so the streaming SDK reads the same structured error.
    let dir = temp_dir("json-stream-err");
    let bad = dir.join("bad.yaml");
    std::fs::write(&bad, "input: [unterminated\n").unwrap();
    let out = Command::new(skilltest())
        .arg("run")
        .arg(&bad)
        .arg("--provider")
        .arg(fake_provider())
        .args([
            "--platform",
            "demo",
            "--model",
            "fake",
            "--format",
            "json-stream",
        ])
        .output()
        .expect("executes");
    assert_eq!(out.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = stdout.lines().last().expect("a terminal NDJSON line");
    let obj: Value = serde_json::from_str(line).expect("the terminal line is JSON");
    assert_eq!(obj["type"], "error");
    assert_eq!(obj["error"]["code"], "usage");
}

#[test]
fn unclassified_provider_error_prints_generic_hint() {
    // A provider command that simply crashes yields an *unclassified* provider
    // error, which prints the generic "install / pass --provider" hint.
    let out = Command::new(skilltest())
        .arg("run")
        .arg(fixtures().join("cases/greet_pass.yaml"))
        .args(["--provider", "/nonexistent/skilltest-provider"])
        .args(["--platform", "demo", "--model", "fake"])
        .output()
        .expect("executes");
    assert_eq!(out.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("on PATH"), "generic hint: {stderr}");
}

#[test]
fn explicit_config_flag_is_loaded() {
    // `--config <path>` takes the explicit-load branch (vs the default lookup).
    let dir = temp_dir("config");
    let cfg = dir.join("custom.yaml");
    std::fs::write(
        &cfg,
        format!(
            "provider:\n  kind: command\n  command: [\"{}\"]\nplatforms: [demo]\nmodels: [fake]\n",
            fake_provider().display()
        ),
    )
    .unwrap();
    let out = Command::new(skilltest())
        .args(["--config", cfg.to_str().unwrap()])
        .arg("run")
        .arg(fixtures().join("cases/greet_pass.yaml"))
        .args(["--format", "json"])
        .output()
        .expect("executes");
    assert!(
        out.status.success(),
        "explicit config run passes: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["passed"], Value::Bool(true));
}

#[test]
fn explicit_config_missing_file_exits_two() {
    let out = Command::new(skilltest())
        .args(["--config", "/no/such/skilltest-config.yaml"])
        .arg("run")
        .arg(fixtures().join("cases/greet_pass.yaml"))
        .output()
        .expect("executes");
    assert_eq!(
        out.status.code(),
        Some(2),
        "missing config is a usage error"
    );
}

#[test]
fn run_human_format_prints_a_summary() {
    // The default (human) format prints the PASS line and the run tally.
    let out = run_passing_case(&[]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("PASS"), "human summary: {stdout}");
    assert!(stdout.contains("runs passed"), "tally: {stdout}");
}

#[test]
fn validate_json_format_reports_findings() {
    let out = Command::new(skilltest())
        .arg("validate")
        .arg(fixtures().join("skills/invalid"))
        .args(["--format", "json"])
        .output()
        .expect("executes");
    // An invalid skill exits 1 even in JSON mode.
    assert_eq!(out.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&out.stdout).expect("valid JSON");
    assert_eq!(report["valid"], Value::Bool(false));
    assert!(!report["findings"].as_array().unwrap().is_empty());
}

#[test]
fn validate_json_format_on_a_good_skill_is_valid() {
    let out = Command::new(skilltest())
        .arg("validate")
        .arg(fixtures().join("skills/greeter"))
        .args(["--format", "json"])
        .output()
        .expect("executes");
    assert!(out.status.success());
    let report: Value = serde_json::from_slice(&out.stdout).expect("valid JSON");
    assert_eq!(report["valid"], Value::Bool(true));
}

#[test]
fn init_into_a_path_under_a_file_is_an_io_error() {
    // Point `init` at a directory whose parent is actually a regular file, so
    // scaffolding's `create_dir_all` fails — exercising the IO error path.
    let dir = temp_dir("init-io");
    let blocker = dir.join("blocker");
    std::fs::write(&blocker, "i am a file, not a dir").unwrap();
    let target = blocker.join("nested"); // parent is a file
    let out = Command::new(skilltest())
        .arg("init")
        .arg(&target)
        .output()
        .expect("executes");
    assert_eq!(out.status.code(), Some(2), "io failure is a usage error");
}

#[test]
fn api_judge_without_key_is_a_classified_auth_error() {
    // Wire the API judge (SplitProvider): the skill runs through the fake
    // command provider, but judging goes to the API judge, which fails with a
    // classified `auth` error when its key env var is unset — exercising the
    // build_provider API-split path and the auth hint, end to end.
    let dir = temp_dir("apijudge");
    let cfg = dir.join("skilltest.yaml");
    std::fs::write(
        &cfg,
        format!(
            "provider:\n  kind: command\n  command: [\"{}\"]\n\
             platforms: [demo]\nmodels: [fake]\n\
             judge:\n  kind: api\n  vendor: anthropic\n  api_key_env: SKILLTEST_DEFINITELY_UNSET\n",
            fake_provider().display()
        ),
    )
    .unwrap();
    let out = Command::new(skilltest())
        .args(["--config", cfg.to_str().unwrap()])
        .arg("run")
        .arg(fixtures().join("cases/greet_pass.yaml"))
        .env_remove("SKILLTEST_DEFINITELY_UNSET")
        .output()
        .expect("executes");
    assert_eq!(
        out.status.code(),
        Some(3),
        "missing API key is a provider error, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("credentials"), "auth hint: {stderr}");
}
