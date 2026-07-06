//! Direct protocol-level tests of the bundled `skilltest-fake-provider` — the
//! deterministic reference provider the e2e suite drives. These pipe one JSON
//! request on stdin and assert on the single JSON response (or the error exit),
//! covering the protocol's happy paths *and* its malformed-input handling, which
//! the framework-level e2e tests don't reach directly.

#![cfg(feature = "fake-provider")]

use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use serde_json::{json, Value};

fn fake_provider() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_skilltest"))
        .parent()
        .expect("binary has a parent dir")
        .join("skilltest-fake-provider")
}

/// Feed `input` to the fake provider on stdin and capture its output.
fn run(input: &str) -> Output {
    let mut child = Command::new(fake_provider())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("fake provider spawns");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().expect("fake provider completes")
}

fn response(input: &str) -> Value {
    let out = run(input);
    assert!(
        out.status.success(),
        "expected success for {input}, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("response is valid JSON")
}

#[test]
fn respond_uses_the_fake_reply_marker() {
    let req = json!({
        "op": "respond",
        "skill": { "name": "s", "path": "/tmp/s", "instructions": "intro\nfake-reply: Hello there\nmore" },
        "messages": [],
    });
    let resp = response(&req.to_string());
    assert_eq!(resp["message"], "Hello there");
    assert_eq!(resp["done"], Value::Bool(false));
    // No fake-tool markers → no events key at all.
    assert!(resp.get("events").is_none());
}

#[test]
fn respond_emits_a_tool_call_event_per_fake_tool_marker() {
    let req = json!({
        "op": "respond",
        "skill": {
            "name": "s",
            "path": "/tmp/s",
            "instructions": "fake-reply: done\nfake-tool: bash git commit -m x\nfake-tool: edit_file config.yaml",
        },
        "messages": [],
    });
    let resp = response(&req.to_string());
    let events = resp["events"].as_array().expect("events array");
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["kind"], "tool_call");
    assert_eq!(events[0]["name"], "bash");
    assert_eq!(events[0]["input"]["command"], "git commit -m x");
    assert_eq!(events[0]["index"], 0);
    assert_eq!(events[1]["name"], "edit_file");
    assert_eq!(events[1]["input"]["command"], "config.yaml");
    assert_eq!(events[1]["index"], 1);
}

#[test]
fn respond_defaults_to_ok_without_a_marker() {
    let req = json!({
        "op": "respond",
        "skill": { "name": "s", "path": "/tmp/s", "instructions": "no marker here" },
        "messages": [],
    });
    assert_eq!(response(&req.to_string())["message"], "ok");
}

#[test]
fn user_uses_the_say_marker_else_continue() {
    let with_marker =
        json!({ "op": "user", "persona": "be terse\nsay: yes please", "messages": [] });
    assert_eq!(response(&with_marker.to_string())["message"], "yes please");
    let without = json!({ "op": "user", "persona": "be terse", "messages": [] });
    assert_eq!(response(&without.to_string())["message"], "continue");
}

#[test]
fn judge_boolean_requires_all_backtick_substrings() {
    let messages = json!([
        { "role": "user", "content": "hi" },
        { "role": "assistant", "content": "Hello, Dr. Smith!" },
    ]);
    // Both required substrings present -> true.
    let present = json!({
        "op": "judge", "kind": "boolean",
        "criterion": "greets `Dr. Smith` and says `Hello`",
        "messages": messages,
    });
    assert_eq!(response(&present.to_string())["value"], Value::Bool(true));
    // A required substring missing -> false.
    let missing = json!({
        "op": "judge", "kind": "boolean",
        "criterion": "mentions `appointment`",
        "messages": messages,
    });
    assert_eq!(response(&missing.to_string())["value"], Value::Bool(false));
}

#[test]
fn judge_numeric_maps_fraction_onto_the_scale() {
    let messages = json!([{ "role": "assistant", "content": "alpha beta" }]);
    // One of two substrings present -> 0.5 of [0, 10] = 5.0.
    let req = json!({
        "op": "judge", "kind": "numeric", "min": 0.0, "max": 10.0,
        "criterion": "has `alpha` and `gamma`",
        "messages": messages,
    });
    let value = response(&req.to_string())["value"].as_f64().unwrap();
    assert!((value - 5.0).abs() < 1e-9, "got {value}");
}

#[test]
fn judge_respects_a_turns_gate() {
    // `turns>=2` is unmet with a single assistant turn -> boolean false.
    let messages = json!([{ "role": "assistant", "content": "only one" }]);
    let req = json!({
        "op": "judge", "kind": "boolean",
        "criterion": "turns>=2",
        "messages": messages,
    });
    assert_eq!(response(&req.to_string())["value"], Value::Bool(false));
}

#[test]
fn invalid_json_request_exits_nonzero_with_message() {
    let out = run("this is not json");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("not valid JSON"), "stderr: {stderr}");
}

#[test]
fn unknown_op_exits_nonzero_with_message() {
    let out = run(&json!({ "op": "frobnicate" }).to_string());
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unknown op"), "stderr: {stderr}");
}

#[test]
fn respond_with_mocks_applies_rules_and_reports_records() {
    // A `mocks` block on the request: the compiled ruleset is applied to each
    // scripted call (first match wins), records carry the ORIGINAL inputs, the
    // stubbed event shows post-rewrite reality, and the canned output/deny
    // message is surfaced into the reply text.
    let request = json!({
        "op": "respond",
        "platform": "demo",
        "model": "fake",
        "skill": {
            "name": "deployer",
            "path": "/tmp/deployer",
            "instructions": "fake-reply: Done.\nfake-tool: bash git push origin\nfake-tool: bash rm -rf /tmp/x\nfake-tool: bash ls\n",
        },
        "messages": [],
        "mocks": { "rules": { "rules": [
            { "match": { "event_contains": "git push" },
              "action": { "stub": { "output": "up-to-date", "exit_code": 0 } } },
            { "match": { "event_contains": "rm -rf" },
              "action": { "deny": { "message": "blocked" } } }
        ]}},
    });
    let resp = response(&request.to_string());

    let records = resp["mock_calls"].as_array().expect("records present");
    assert_eq!(records.len(), 3);
    assert_eq!(records[0]["action"], "stub");
    assert_eq!(records[0]["rule"], 0);
    assert_eq!(records[0]["input"]["command"], "git push origin");
    assert_eq!(records[1]["action"], "deny");
    assert_eq!(records[1]["rule"], 1);
    assert_eq!(records[2]["action"], "allow");
    assert!(records[2]["rule"].is_null());

    // Events: the stub became a printf (post-rewrite), the deny carries the
    // message as output, the allowed call is untouched.
    let events = resp["events"].as_array().unwrap();
    let stub_cmd = events[0]["input"]["command"].as_str().unwrap();
    assert!(stub_cmd.starts_with("printf"), "{stub_cmd}");
    assert_eq!(events[0]["output"], "up-to-date\n");
    assert_eq!(events[1]["output"], "denied: blocked");
    assert_eq!(events[2]["input"]["command"], "ls");

    // The reply text surfaces what the model "saw" from the intercepts.
    let message = resp["message"].as_str().unwrap();
    assert!(message.contains("up-to-date"), "{message}");
    assert!(message.contains("denied: blocked"), "{message}");
}

#[test]
fn respond_spy_only_mocks_block_records_allows() {
    // `"rules": null` (a spy-only run): everything is allowed through, but
    // every call is still recorded — and `mock_calls` is present-but-empty for
    // a turn with no tool calls, never absent.
    let request = json!({
        "op": "respond",
        "platform": "demo",
        "model": "fake",
        "skill": { "name": "s", "path": "/tmp/s",
                   "instructions": "fake-reply: hi\nfake-tool: bash ls\n" },
        "messages": [],
        "mocks": { "rules": null },
    });
    let resp = response(&request.to_string());
    let records = resp["mock_calls"].as_array().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["action"], "allow");

    let no_tools = json!({
        "op": "respond",
        "platform": "demo",
        "model": "fake",
        "skill": { "name": "s", "path": "/tmp/s", "instructions": "fake-reply: hi\n" },
        "messages": [],
        "mocks": { "rules": null },
    });
    let resp = response(&no_tools.to_string());
    assert_eq!(resp["mock_calls"], json!([]));
    // And with no `mocks` block at all, the field never appears.
    let unmocked = json!({
        "op": "respond",
        "platform": "demo",
        "model": "fake",
        "skill": { "name": "s", "path": "/tmp/s", "instructions": "fake-reply: hi\n" },
        "messages": [],
    });
    let resp = response(&unmocked.to_string());
    assert!(resp.get("mock_calls").is_none());
}
