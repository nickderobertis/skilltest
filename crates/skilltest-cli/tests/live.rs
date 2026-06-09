//! Live-provider smoke test. This is the one sanctioned use of `#[ignore]` in
//! the repo (see `references/languages/rust.md` and `tests/AGENTS.md`): it needs
//! a **real** provider (e.g. `oneharness`) and model credentials, which must
//! never be in the deterministic gate. It is therefore opt-in only:
//!
//! ```bash
//! SKILLTEST_LIVE_PROVIDER=oneharness \
//!   cargo test -p skilltest-cli --test live -- --ignored
//! ```
//!
//! Optional knobs: `SKILLTEST_LIVE_PLATFORM`, `SKILLTEST_LIVE_MODEL`.
//!
//! The default gate skips this (nextest/`cargo test` do not run ignored tests),
//! so CI stays deterministic while there is still a one-command way to smoke a
//! real provider before a release.

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

fn skilltest() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_skilltest"))
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

#[test]
#[ignore = "needs a live provider; set SKILLTEST_LIVE_PROVIDER and run with --ignored"]
fn live_provider_runs_a_case() {
    let provider = std::env::var("SKILLTEST_LIVE_PROVIDER")
        .expect("set SKILLTEST_LIVE_PROVIDER to the provider command (e.g. oneharness)");
    let platform =
        std::env::var("SKILLTEST_LIVE_PLATFORM").unwrap_or_else(|_| "claude-code".into());
    let model = std::env::var("SKILLTEST_LIVE_MODEL").unwrap_or_else(|_| "claude-opus-4-8".into());

    let out = Command::new(skilltest())
        .arg("run")
        .arg(fixtures().join("cases/greet_pass.yaml"))
        .args(["--provider", &provider])
        .args(["--platform", &platform])
        .args(["--model", &model])
        .args(["--format", "json"])
        .output()
        .expect("skilltest run executes");

    // A real model is non-deterministic, so we don't assert pass/fail — only that
    // the provider was reachable (exit 0 = passed or 1 = an eval failed; not 3)
    // and that we got a well-formed report back.
    let code = out.status.code();
    assert!(
        code == Some(0) || code == Some(1),
        "expected a completed run (0/1), got {code:?}; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: Value = serde_json::from_slice(&out.stdout).expect("stdout is a JSON report");
    assert!(
        report["runs"].as_array().is_some_and(|r| !r.is_empty()),
        "report contains at least one run"
    );
}
