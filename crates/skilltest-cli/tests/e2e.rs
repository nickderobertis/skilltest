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
    assert_eq!(report["summary"]["runs"], 8);
    assert!(report["summary"]["failed"].as_u64().unwrap() >= 1);
}

#[test]
fn tool_events_surface_on_the_assistant_turn() {
    // The fake provider emits a normalized `tool_call` per `fake-tool:` marker in
    // the skill; the runner lifts them onto the assistant message so consumers
    // can analyze what the skill *did*, not just what it said.
    let out = run_case(case("tool_events.yaml"), &["--format", "json"]);
    assert!(out.status.success(), "expected exit 0");
    let report = json(&out);
    let messages = report["runs"][0]["transcript"]["messages"]
        .as_array()
        .expect("transcript has messages");
    let assistant = messages
        .iter()
        .find(|m| m["role"] == "assistant")
        .expect("an assistant turn");
    let events = assistant["events"].as_array().expect("events array");
    assert_eq!(events.len(), 2, "one event per fake-tool marker");
    assert_eq!(events[0]["kind"], "tool_call");
    assert_eq!(events[0]["name"], "edit_file");
    assert_eq!(events[0]["input"]["command"], "config.yaml");
    assert_eq!(events[1]["name"], "bash");
    assert_eq!(
        events[1]["input"]["command"],
        "git commit -m \"update config\""
    );
}

#[test]
fn json_stream_emits_events_then_a_terminal_result() {
    // The streaming format the SDKs consume: one NDJSON `event` line per tool
    // event as it happens, then a terminal `result` line with the full report.
    let out = run_case(case("tool_events.yaml"), &["--format", "json-stream"]);
    assert!(out.status.success(), "expected exit 0");
    let text = String::from_utf8(out.stdout).expect("stdout is utf8");
    let lines: Vec<Value> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("each line is one JSON object"))
        .collect();
    assert_eq!(lines.len(), 3, "two events then a result");

    assert_eq!(lines[0]["type"], "event");
    assert_eq!(lines[0]["case"], "tool_events");
    assert_eq!(lines[0]["turn"], 1);
    assert_eq!(lines[0]["event"]["name"], "edit_file");
    assert_eq!(lines[1]["type"], "event");
    assert_eq!(lines[1]["event"]["name"], "bash");

    assert_eq!(lines[2]["type"], "result");
    assert_eq!(lines[2]["report"]["passed"], Value::Bool(true));
    // The terminal report carries the same events on the transcript.
    let messages = lines[2]["report"]["runs"][0]["transcript"]["messages"]
        .as_array()
        .unwrap();
    let assistant = messages.iter().find(|m| m["role"] == "assistant").unwrap();
    assert_eq!(assistant["events"].as_array().unwrap().len(), 2);
}

#[test]
fn json_stream_short_circuits_when_the_consumer_stops_reading() {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};
    use std::sync::mpsc;
    use std::time::Duration;

    // The manytools case emits thousands of events — far more than the stdout
    // pipe buffer holds — so a consumer that reads one line and stops leaves the
    // CLI mid-write. Its broken-pipe path must then tear the run down and exit,
    // rather than block forever.
    let case = fixtures().join("stream/manytools.yaml");
    let mut child = Command::new(skilltest())
        .arg("run")
        .arg(&case)
        .args(["--format", "json-stream"])
        .arg("--provider")
        .arg(fake_provider())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("skilltest spawns");

    {
        // Read a single event line, then drop the reader (close the read end).
        let stdout = child.stdout.take().expect("piped stdout");
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader.read_line(&mut line).expect("reads one line");
        assert!(
            line.contains("\"type\":\"event\""),
            "first line is an event: {line}"
        );
    }

    // Wait for exit, with a watchdog so a hang fails the test instead of stalling
    // the whole suite.
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait().map(|s| s.success()));
    });
    match rx.recv_timeout(Duration::from_secs(20)) {
        Ok(_) => {} // exited promptly once the consumer closed the stream
        Err(_) => panic!("CLI did not terminate after the consumer closed the stream"),
    }
}

// ---------------------------------------------------------------------------
// Code-defined cases: the `--case-json` ingestion path the SDKs use to run a
// case built in code (no YAML file on disk). Same conversation loop, evals,
// and JSON contract — only the case's origin differs.
// ---------------------------------------------------------------------------

/// Write `json` to a temp `--case-json` file and run it against the fake
/// provider, returning the process output.
fn run_case_json(json: &str, extra: &[&str]) -> Output {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    // A per-call unique tag: these tests run in parallel, so a shared dir would
    // let one case.json clobber another's.
    let dir = unique_dir(&format!("case-json-{}", N.fetch_add(1, Ordering::Relaxed)));
    let file = dir.join("case.json");
    std::fs::write(&file, json).unwrap();
    let mut cmd = Command::new(skilltest());
    cmd.arg("run")
        .arg("--case-json")
        .arg(&file)
        .arg("--provider")
        .arg(fake_provider())
        .args(["--platform", "demo", "--model", "fake"])
        .args(extra);
    cmd.output().expect("skilltest run executes")
}

#[test]
fn case_json_single_object_runs_like_a_yaml_case() {
    let skill = fixtures().join("skills/greeter");
    let body = format!(
        r#"{{"name":"inline_greet","skill":{skill:?},"input":"Greet Dr. Smith",
            "evals":[{{"type":"boolean","criterion":"the reply greets `Dr. Smith` by name"}}]}}"#,
        skill = skill.to_str().unwrap()
    );
    let out = run_case_json(&body, &["--format", "json"]);
    assert!(
        out.status.success(),
        "expected exit 0; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report = json(&out);
    assert_eq!(report["passed"], Value::Bool(true));
    assert_eq!(report["runs"][0]["case"], "inline_greet");
    assert_eq!(report["runs"][0]["turns"], 1);
}

#[test]
fn case_json_array_runs_every_case() {
    let skill = fixtures().join("skills/greeter");
    let s = skill.to_str().unwrap();
    let body = format!(
        r#"[
            {{"skill":{s:?},"input":"Greet Dr. Smith",
              "evals":[{{"type":"boolean","criterion":"greets `Dr. Smith`"}}]}},
            {{"name":"second","skill":{s:?},"input":"Greet Dr. Smith",
              "evals":[{{"type":"boolean","criterion":"confirms `confirmed`"}}]}}
        ]"#
    );
    let out = run_case_json(&body, &["--format", "json"]);
    assert!(out.status.success(), "expected exit 0");
    let report = json(&out);
    assert_eq!(report["summary"]["runs"], 2);
    // Unnamed first case defaults to `case`; the second keeps its name.
    let names: Vec<&str> = report["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["case"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"case") && names.contains(&"second"),
        "{names:?}"
    );
}

#[test]
fn case_json_resolves_relative_skill_against_the_working_directory() {
    // A code-defined case's `skill` resolves relative to CWD (the SDKs run the
    // CLI from the user's project), not to the temp file the JSON lives in.
    let dir = unique_dir("case-json-cwd");
    let file = dir.join("case.json");
    std::fs::write(
        &file,
        r#"{"skill":"skills/greeter","input":"Greet Dr. Smith",
            "evals":[{"type":"boolean","criterion":"greets `Dr. Smith`"}]}"#,
    )
    .unwrap();
    let out = Command::new(skilltest())
        .current_dir(fixtures())
        .arg("run")
        .arg("--case-json")
        .arg(&file)
        .arg("--provider")
        .arg(fake_provider())
        .args(["--platform", "demo", "--model", "fake", "--format", "json"])
        .output()
        .expect("executes");
    assert!(
        out.status.success(),
        "expected exit 0; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(json(&out)["passed"], Value::Bool(true));
}

#[test]
fn case_json_failing_eval_exits_one() {
    let skill = fixtures().join("skills/greeter");
    let body = format!(
        r#"{{"skill":{s:?},"input":"Greet Dr. Smith",
            "evals":[{{"type":"boolean","criterion":"mentions `nonexistent phrase`"}}]}}"#,
        s = skill.to_str().unwrap()
    );
    let out = run_case_json(&body, &["--format", "json"]);
    assert_eq!(out.status.code(), Some(1), "a failing eval exits 1");
    assert_eq!(json(&out)["passed"], Value::Bool(false));
}

/// A minimal one-eval case JSON against the greeter fixture, named `name`.
fn greeter_case_json(name: &str) -> String {
    let skill = fixtures().join("skills/greeter");
    format!(
        r#"{{"name":{name:?},"skill":{s:?},"input":"Greet Dr. Smith",
            "evals":[{{"type":"boolean","criterion":"greets `Dr. Smith`"}}]}}"#,
        s = skill.to_str().unwrap()
    )
}

#[test]
fn case_json_combines_with_positional_yaml_paths() {
    // The two ingestion channels compose in one run: a positional YAML PATH
    // and a --case-json file both contribute cases to the same report.
    let dir = unique_dir("case-json-combined");
    let file = dir.join("inline.json");
    std::fs::write(&file, greeter_case_json("from_json")).unwrap();
    let out = Command::new(skilltest())
        .arg("run")
        .arg(case("greet_pass.yaml"))
        .arg("--case-json")
        .arg(&file)
        .arg("--provider")
        .arg(fake_provider())
        .args(["--platform", "demo", "--model", "fake", "--format", "json"])
        .output()
        .expect("executes");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report = json(&out);
    assert_eq!(report["summary"]["runs"], 2);
    let names: Vec<&str> = report["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["case"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"greet_pass") && names.contains(&"from_json"),
        "{names:?}"
    );
}

#[test]
fn case_json_flag_is_repeatable() {
    let dir = unique_dir("case-json-repeat");
    let first = dir.join("a.json");
    let second = dir.join("b.json");
    std::fs::write(&first, greeter_case_json("first")).unwrap();
    std::fs::write(&second, greeter_case_json("second")).unwrap();
    let out = Command::new(skilltest())
        .arg("run")
        .args(["--case-json".as_ref(), first.as_os_str()])
        .args(["--case-json".as_ref(), second.as_os_str()])
        .arg("--provider")
        .arg(fake_provider())
        .args(["--platform", "demo", "--model", "fake", "--format", "json"])
        .output()
        .expect("executes");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(json(&out)["summary"]["runs"], 2);
}

#[test]
fn case_json_malformed_exits_two() {
    let out = run_case_json("{ not valid json", &["--format", "json"]);
    assert_eq!(out.status.code(), Some(2), "bad input exits 2");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("case-json"), "stderr explains: {stderr}");
}

#[test]
fn yaml_case_with_typoed_eval_field_exits_two_naming_the_field() {
    // The same strictness through the YAML ingestion path: a typo'd key inside
    // an eval aborts the run with the field named.
    let dir = unique_dir("yaml-eval-typo");
    let case_path = dir.join("typo.yaml");
    std::fs::write(
        &case_path,
        format!(
            "skill: {}\ninput: hi\nevals:\n  - type: boolean\n    criterion: greets\n    expcted: false\n",
            fixtures().join("skills/greeter").display()
        ),
    )
    .unwrap();
    let out = run_case(case_path, &["--format", "json"]);
    assert_eq!(out.status.code(), Some(2), "typo'd eval field exits 2");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("expcted"),
        "stderr names the field: {stderr}"
    );
}

#[test]
fn case_json_typoed_eval_field_exits_two_naming_the_field() {
    // A typo'd eval key must be a loud usage error naming the field — never a
    // silently-applied default that could invert the eval's intent.
    let skill = fixtures().join("skills/greeter");
    let body = format!(
        r#"{{"skill":{s:?},"input":"Greet Dr. Smith",
            "evals":[{{"type":"boolean","criterion":"greets","expcted":false}}]}}"#,
        s = skill.to_str().unwrap()
    );
    let out = run_case_json(&body, &["--format", "json"]);
    assert_eq!(out.status.code(), Some(2), "typo'd eval field exits 2");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("expcted"),
        "stderr names the field: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// The input contract: `schemas/case.schema.json` is the golden the SDKs'
// generated case models come from, and the kitchen-sink fixture is the shared
// JSON every party pins — Rust serialization here, and each SDK's case
// builders in their own suites. Drift in any direction breaks a named test.
// ---------------------------------------------------------------------------

/// Path to the shared kitchen-sink case golden.
fn kitchen_sink_path() -> PathBuf {
    fixtures().join("contract/case_kitchen_sink.json")
}

#[test]
fn kitchen_sink_golden_matches_rust_construction() {
    use skilltest_core::{
        BooleanEval, CalledEval, Comparator, DenySpec, Eval, FieldPredicate, FieldPredicateSpec,
        MockDecl, MockMatch, NotCalledEval, NumericEval, SimulatedUser, StubOutput, StubSpec,
        TestCase,
    };
    use std::collections::BTreeMap;

    let contains = |needle: &str| {
        FieldPredicate::Spec(FieldPredicateSpec {
            equals: None,
            contains: Some(needle.to_string()),
            pattern: None,
        })
    };
    let where_origin: BTreeMap<String, FieldPredicate> =
        [("command".to_string(), contains("origin"))].into();

    // A maximal case exercising every field of the input contract. If a field
    // is added to the Rust types, this construction (and the golden) must grow
    // with it — which in turn fails the SDK builder golden tests until the
    // builders can express it.
    let case = TestCase {
        name: "kitchen_sink".into(),
        skill: "tests/fixtures/skills/deployer".into(),
        input: "Deploy the app".into(),
        user: Some(SimulatedUser {
            persona: "a terse operator\nsay: Yes, proceed.".into(),
            done_when: Some("the conversation has reached turns>=1".into()),
            max_turns: Some(3),
        }),
        mocks: vec![
            MockDecl {
                name: Some("push".into()),
                matcher: MockMatch {
                    tool: Some("bash".into()),
                    contains: None,
                    pattern: Some("git push( --force)?\\b".into()),
                    input: where_origin.clone(),
                },
                stub: Some(StubSpec::Full(StubOutput {
                    output: "Everything up-to-date".into(),
                    exit_code: 0,
                })),
                deny: None,
                rewrite: None,
            },
            MockDecl {
                name: Some("danger".into()),
                matcher: MockMatch {
                    tool: None,
                    contains: Some("rm -rf".into()),
                    pattern: None,
                    input: BTreeMap::new(),
                },
                stub: None,
                deny: Some(DenySpec::Text("destructive commands are blocked".into())),
                rewrite: None,
            },
            MockDecl {
                name: Some("status".into()),
                matcher: MockMatch {
                    tool: None,
                    contains: None,
                    pattern: None,
                    input: [(
                        "command".to_string(),
                        FieldPredicate::Equals("git status".into()),
                    )]
                    .into(),
                },
                stub: None,
                deny: None,
                rewrite: Some(serde_json::json!({ "command": "git status --short" })),
            },
            MockDecl {
                name: Some("sudo".into()),
                matcher: MockMatch {
                    tool: Some("bash".into()),
                    contains: Some("sudo".into()),
                    pattern: None,
                    input: BTreeMap::new(),
                },
                stub: None,
                deny: None,
                rewrite: None,
            },
        ],
        spy: true,
        evals: vec![
            Eval::Boolean(BooleanEval {
                criterion: "the reply mentions `flying pigs`".into(),
                expected: false,
                name: Some("no-nonsense".into()),
            }),
            Eval::Numeric(NumericEval {
                criterion: "mentions `Deployment finished.`".into(),
                min: 0.0,
                max: 10.0,
                threshold: 5.0,
                comparator: Comparator::Gt,
                name: Some("finished".into()),
            }),
            Eval::Called(CalledEval {
                mock: "push".into(),
                times: Some(1),
                r#where: where_origin,
                name: Some("pushed-once".into()),
            }),
            Eval::NotCalled(NotCalledEval {
                mock: "sudo".into(),
                r#where: BTreeMap::new(),
                name: Some("no-sudo".into()),
            }),
        ],
    };

    let golden: Value = serde_json::from_str(
        &std::fs::read_to_string(kitchen_sink_path()).expect("kitchen-sink golden exists"),
    )
    .expect("golden is valid JSON");
    let serialized = serde_json::to_value(&case).expect("case serializes");
    assert_eq!(
        serialized, golden,
        "tests/fixtures/contract/case_kitchen_sink.json is out of date with the Rust input \
         types — update the golden and every SDK's case builders together"
    );

    // And the golden round-trips through the strict parse.
    let parsed: TestCase = serde_json::from_value(golden).expect("golden parses strictly");
    assert_eq!(parsed, case);
}

#[test]
fn kitchen_sink_golden_runs_and_passes() {
    // The golden is executable, not just parseable: from the repo root (its
    // `skill` is repo-relative) it runs against the fake provider and every
    // eval kind passes.
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = Command::new(skilltest())
        .current_dir(&repo_root)
        .arg("run")
        .arg("--case-json")
        .arg(kitchen_sink_path())
        .arg("--provider")
        .arg(fake_provider())
        .args(["--platform", "demo", "--model", "fake", "--format", "json"])
        .output()
        .expect("executes");
    assert!(
        out.status.success(),
        "kitchen sink passes; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report = json(&out);
    assert_eq!(report["runs"][0]["case"], "kitchen_sink");
    assert_eq!(report["runs"][0]["evals"].as_array().unwrap().len(), 4);
}

#[test]
fn case_json_missing_file_exits_two() {
    let missing =
        std::env::temp_dir().join(format!("skilltest-nocasejson-{}.json", std::process::id()));
    let out = Command::new(skilltest())
        .arg("run")
        .arg("--case-json")
        .arg(&missing)
        .args(["--provider", "true"])
        .output()
        .expect("executes");
    assert_eq!(
        out.status.code(),
        Some(2),
        "an unreadable --case-json exits 2"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("case-json"), "stderr explains: {stderr}");
}

#[test]
fn run_with_no_cases_is_a_usage_error() {
    // Neither a positional PATH nor --case-json: a loud usage error, not a
    // vacuous success.
    let out = Command::new(skilltest())
        .arg("run")
        .args(["--provider", "true"])
        .output()
        .expect("executes");
    assert_eq!(out.status.code(), Some(2), "no cases exits 2");
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

/// Assert that `skilltest schema <target>` matches the golden in `schemas/`.
/// The goldens are what the SDK models are generated from, so drift here means
/// the Rust types changed without regenerating the contract.
fn assert_schema_matches_golden(target: &str, golden: &str) {
    let out = Command::new(skilltest())
        .args(["schema", target])
        .output()
        .expect("executes");
    assert!(out.status.success(), "schema {target} exits 0");
    let emitted: Value = serde_json::from_slice(&out.stdout).expect("schema is valid JSON");
    let golden_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas")
        .join(golden);
    let golden_text = std::fs::read_to_string(&golden_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", golden_path.display()));
    let golden_json: Value = serde_json::from_str(&golden_text).expect("golden is valid JSON");
    assert_eq!(
        emitted, golden_json,
        "schemas/{golden} is out of date with the Rust report types — run `just gen-contract` \
         and commit the regenerated artifacts"
    );
}

#[test]
fn schema_report_matches_checked_in_golden() {
    assert_schema_matches_golden("report", "report.schema.json");
}

#[test]
fn schema_validation_matches_checked_in_golden() {
    assert_schema_matches_golden("validation", "validation.schema.json");
}

#[test]
fn schema_case_matches_checked_in_golden() {
    assert_schema_matches_golden("case", "case.schema.json");
}

#[test]
fn schema_error_matches_checked_in_golden() {
    assert_schema_matches_golden("error", "error.schema.json");
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

/// A fresh, unique temp directory for a test.
fn unique_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("skilltest-e2e-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn init_scaffolds_a_runnable_project() {
    let dir = unique_dir("init");
    let out = Command::new(skilltest())
        .arg("init")
        .arg(&dir)
        .output()
        .expect("executes");
    assert!(out.status.success(), "init exits 0");
    assert!(dir.join("skilltest.yaml").is_file());
    assert!(dir.join("skills/example/SKILL.md").is_file());
    assert!(dir.join("cases/example.yaml").is_file());

    // The scaffolded skill validates, and the scaffolded case runs and passes
    // offline against the fake provider — proving the starter project works.
    let validated = Command::new(skilltest())
        .arg("validate")
        .arg(dir.join("skills/example"))
        .output()
        .expect("executes");
    assert!(validated.status.success(), "scaffolded skill validates");

    let ran = run_case(dir.join("cases/example.yaml"), &["--format", "json"]);
    assert!(ran.status.success(), "scaffolded case passes offline");
    assert_eq!(json(&ran)["passed"], Value::Bool(true));
}

#[test]
fn init_refuses_to_overwrite() {
    let dir = unique_dir("init-clobber");
    let first = Command::new(skilltest())
        .arg("init")
        .arg(&dir)
        .output()
        .expect("executes");
    assert!(first.status.success());
    let second = Command::new(skilltest())
        .arg("init")
        .arg(&dir)
        .output()
        .expect("executes");
    assert_eq!(second.status.code(), Some(2), "re-init refuses with exit 2");
    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(
        stderr.contains("overwrite"),
        "explains the refusal: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// Tool mocking and spying (the `mocks:` block, `--mocks`, `--spy`, and the
// deterministic `called`/`not_called` evals) — driven end to end through the
// fake provider, which applies the same compiled ruleset the oneharness hook
// would (one shared decision engine in skilltest-core).
// ---------------------------------------------------------------------------

#[test]
fn mock_stub_intercepts_and_call_evals_pass() {
    let out = run_case(case("mock_stub.yaml"), &["--format", "json"]);
    assert!(
        out.status.success(),
        "expected exit 0; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report = json(&out);
    assert_eq!(report["passed"], Value::Bool(true));

    // The channel recorded every call with its ORIGINAL input and verdict; the
    // intercepting rule is resolved to its mock's name.
    let records = report["runs"][0]["mock_calls"]
        .as_array()
        .expect("mock_calls present when mocks are declared");
    assert_eq!(records.len(), 3, "push + status + rm: {records:?}");
    assert_eq!(records[0]["action"], "stub");
    assert_eq!(records[0]["mock"], "push");
    assert_eq!(records[0]["input"]["command"], "git push origin main");
    assert_eq!(records[1]["action"], "allow");
    assert!(records[1]["mock"].is_null());

    // Transcript events show post-rewrite reality: the stubbed call became the
    // safely-quoted printf, while the spy log (above) kept the original.
    let events = report["runs"][0]["transcript"]["messages"][1]["events"]
        .as_array()
        .expect("assistant turn carries events");
    let stubbed = events[0]["input"]["command"].as_str().unwrap();
    assert!(
        stubbed.starts_with("printf") && stubbed.contains("Everything up-to-date"),
        "post-rewrite event shows the stub: {stubbed}"
    );

    // The canned output surfaced to the model (the boolean eval judged it) and
    // the deterministic evals carry the calls detail.
    let evals = report["runs"][0]["evals"].as_array().unwrap();
    assert_eq!(evals.len(), 4);
    assert_eq!(evals[1]["detail"]["kind"], "calls");
    assert_eq!(evals[1]["detail"]["count"], 1);
    assert_eq!(evals[3]["detail"]["negated"], true);
}

#[test]
fn mock_violation_fails_not_called_and_reports_the_call() {
    let out = run_case(case("mock_violation.yaml"), &["--format", "json"]);
    assert_eq!(out.status.code(), Some(1), "a violated expectation exits 1");
    let report = json(&out);
    assert_eq!(report["passed"], Value::Bool(false));
    let eval = &report["runs"][0]["evals"][0];
    assert_eq!(eval["passed"], Value::Bool(false));
    assert_eq!(eval["detail"]["kind"], "calls");
    assert_eq!(eval["detail"]["count"], 1);
    assert_eq!(eval["detail"]["negated"], true);
    // The failure reason shows the offending call and its verdict, so the
    // report alone diagnoses it.
    let reason = eval["reason"].as_str().unwrap();
    assert!(
        reason.contains("rm -rf") && reason.contains("[deny]"),
        "reason: {reason}"
    );
}

#[test]
fn invalid_mock_pattern_is_a_usage_error() {
    // An invalid regex must abort at load (exit 2) — never degrade to a rule
    // that silently matches nothing.
    let dir = unique_dir("mock-badregex");
    let case_path = dir.join("bad.yaml");
    std::fs::write(
        &case_path,
        format!(
            "skill: {}\ninput: deploy\nmocks:\n  - name: bad\n    match: {{ pattern: \"git push(\" }}\n    stub: x\nevals:\n  - type: boolean\n    criterion: ok\n",
            fixtures().join("skills/deployer").display()
        ),
    )
    .unwrap();
    let out = run_case(case_path, &["--format", "json"]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("not a valid regex"), "stderr: {stderr}");
}

#[test]
fn unknown_mock_reference_is_a_usage_error_listing_names() {
    // A `called` eval naming a mock that doesn't exist is a loud usage error
    // that lists what is declared — a typo must never match nothing.
    let dir = unique_dir("mock-unknown");
    let case_path = dir.join("typo.yaml");
    std::fs::write(
        &case_path,
        format!(
            "skill: {}\ninput: deploy\nmocks:\n  - name: push\n    match: {{ contains: \"git push\" }}\n    stub: ok\nevals:\n  - type: called\n    mock: psuh\n",
            fixtures().join("skills/deployer").display()
        ),
    )
    .unwrap();
    let out = run_case(case_path, &["--format", "json"]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unknown mock `psuh`") && stderr.contains("push"),
        "stderr: {stderr}"
    );
}

#[test]
fn cli_mocks_file_applies_shared_declarations_to_every_case() {
    // Code-level mocks (what the SDKs send) ride `--mocks <file>`: the case
    // declares none itself, yet the shared stub intercepts and its name
    // resolves for the case's `called` eval.
    let dir = unique_dir("mock-clifile");
    let mocks_path = dir.join("mocks.yaml");
    std::fs::write(
        &mocks_path,
        "- name: push\n  match: { tool: bash, pattern: \"git push( --force)?\\\\b\" }\n  stub: Everything up-to-date\n",
    )
    .unwrap();
    let case_path = dir.join("shared.yaml");
    std::fs::write(
        &case_path,
        format!(
            "skill: {}\ninput: deploy\nevals:\n  - type: called\n    mock: push\n    times: 1\n",
            fixtures().join("skills/deployer").display()
        ),
    )
    .unwrap();
    let out = run_case(
        case_path,
        &["--mocks", mocks_path.to_str().unwrap(), "--format", "json"],
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report = json(&out);
    assert_eq!(report["runs"][0]["mock_calls"][0]["mock"], "push");
}

#[test]
fn spy_flag_records_calls_without_any_mocks() {
    // `--spy` turns the observation channel on for a case with no mocks: the
    // report gains `mock_calls` (all `allow`), and without the flag the field
    // stays null — "channel off" and "zero calls" are distinguishable.
    let with_spy = run_case(case("tool_events.yaml"), &["--spy", "--format", "json"]);
    assert!(with_spy.status.success());
    let report = json(&with_spy);
    let records = report["runs"][0]["mock_calls"].as_array().unwrap();
    assert!(!records.is_empty());
    assert!(records.iter().all(|r| r["action"] == "allow"));

    let without = run_case(case("tool_events.yaml"), &["--format", "json"]);
    assert!(json(&without)["runs"][0]["mock_calls"].is_null());
}

// ---------------------------------------------------------------------------
// Run history: the OneharnessProvider path can't be reached through the fake
// *provider* (a CommandProvider records no history), so these drive the built
// CLI against a fake *oneharness* binary via `--oneharness-bin` — the same
// process-boundary fake the provider unit tests use — to prove the whole
// built-binary path: default oneharness provider → --history flags → the
// report's `history_command` and the human `history:` line. Unix-only, like the
// provider subprocess suite (the crate ships to a Linux/macOS matrix).
// ---------------------------------------------------------------------------

/// A fake `oneharness` that echoes a recorded `history_file` on skill runs (the
/// ones carrying `--history`) and a JSON verdict on judge/user runs (which do
/// not). Written to a fresh temp dir; returns (binary path, history dir).
#[cfg(unix)]
fn fake_oneharness(tag: &str) -> (PathBuf, PathBuf) {
    use std::io::Write as _;
    use std::os::unix::fs::PermissionsExt as _;
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "skilltest-e2e-oh-{}-{tag}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let history = dir.join("history");
    std::fs::create_dir_all(&history).unwrap();
    let path = dir.join("oneharness.sh");
    let body = "#!/bin/sh\n\
        args=\"$*\"\n\
        cat >/dev/null\n\
        case \"$args\" in\n\
          *--history*) printf '%s\\n' '{\"results\":[{\"status\":\"ok\",\"text\":\"Hello, Dr. Smith!\"}],\"history_file\":\"/x/s.jsonl\"}' ;;\n\
          *) printf '%s\\n' '{\"results\":[{\"status\":\"ok\",\"text\":\"{\\\"value\\\": true, \\\"reason\\\": \\\"names her\\\"}\"}]}' ;;\n\
        esac\n";
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(body.as_bytes()).unwrap();
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).unwrap();
    (path, history)
}

/// Run `skilltest run <case>` against a fake oneharness, with the centralized
/// history dir pinned to `history_dir` so the run is hermetic and assertable.
#[cfg(unix)]
fn run_via_fake_oneharness(
    oh: &std::path::Path,
    history_dir: &std::path::Path,
    extra: &[&str],
) -> Output {
    let mut cmd = Command::new(skilltest());
    cmd.arg("run")
        .arg(case("greet_pass.yaml"))
        .args(["--oneharness-bin", &oh.to_string_lossy()])
        .args(["--platform", "claude-code", "--model", "sonnet"])
        .env("SKILLTEST_HISTORY_DIR", history_dir)
        .args(extra);
    cmd.output().expect("skilltest run executes")
}

#[cfg(unix)]
#[test]
fn oneharness_history_command_in_report_and_human_output() {
    let (oh, history) = fake_oneharness("on");

    // JSON: the run carries a runnable `history show` command pointed at the
    // centralized dir this invocation used.
    let out = run_via_fake_oneharness(&oh, &history, &["--format", "json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report = json(&out);
    let cmd = report["runs"][0]["history_command"]
        .as_str()
        .expect("history_command present");
    assert!(
        cmd.contains("history show skilltest-claude-code-sonnet-"),
        "cmd: {cmd}"
    );
    assert!(
        cmd.contains(&format!("--history-dir {}", history.display())),
        "cmd: {cmd}"
    );

    // Human: the same command surfaces as a `history:` line.
    let human = run_via_fake_oneharness(&oh, &history, &[]);
    assert!(human.status.success());
    let text = String::from_utf8_lossy(&human.stdout);
    assert!(text.contains("history: "), "human output:\n{text}");
    assert!(
        text.contains("history show skilltest-claude-code-sonnet-"),
        "human output:\n{text}"
    );
}

#[cfg(unix)]
#[test]
fn oneharness_history_can_be_disabled_via_config() {
    let (oh, history) = fake_oneharness("off");
    // A config that turns recording off: no history flags go out (so the fake
    // returns no history_file) and no command is surfaced.
    let cfg = oh.parent().unwrap().join("skilltest.yaml");
    std::fs::write(&cfg, "provider:\n  kind: oneharness\n  history: false\n").unwrap();
    let mut cmd = Command::new(skilltest());
    let out = cmd
        .args(["--config", &cfg.to_string_lossy()])
        .arg("run")
        .arg(case("greet_pass.yaml"))
        .args(["--oneharness-bin", &oh.to_string_lossy()])
        .args([
            "--platform",
            "claude-code",
            "--model",
            "sonnet",
            "--format",
            "json",
        ])
        .env("SKILLTEST_HISTORY_DIR", &history)
        .output()
        .expect("skilltest run executes");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        json(&out)["runs"][0]["history_command"].is_null(),
        "history was disabled, so no command should be surfaced"
    );
}
