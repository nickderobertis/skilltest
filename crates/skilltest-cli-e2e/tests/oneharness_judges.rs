//! Report-boundary journeys for metadata combinations a third-party producer may emit.
#![cfg(unix)]
mod common;
use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "skilltest-judges-{}-{}",
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).unwrap();
        Self(dir)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn journey(result: Value, numeric: bool, exit: i32) -> (Scratch, std::process::Output) {
    let scratch = Scratch::new();
    std::fs::write(
        scratch.0.join("judge.json"),
        json!({"results":[result]}).to_string(),
    )
    .unwrap();
    let boundary = scratch.0.join("oneharness");
    std::fs::write(
        &boundary,
        r#"#!/bin/sh
schema=''
while [ "$#" -gt 0 ]; do
  case "$1" in --schema) schema="$2"; shift ;; esac
  shift
done
cat >/dev/null
if [ -n "$schema" ]; then
  cp "$schema" "$JOURNEY_DIR/schema.json"
  printf '%s' "$schema" > "$JOURNEY_DIR/schema-path"
  cat "$JOURNEY_DIR/judge.json"
  exit "$JUDGE_EXIT"
fi
printf '%s\n' '{"results":[{"status":"ok","text":"Hello Dr. Smith, appointment confirmed"}]}'
"#,
    )
    .unwrap();
    std::fs::set_permissions(&boundary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    let output = Command::new(common::built_bin("skilltest"))
        .arg("run")
        .arg(fixtures.join(if numeric {
            "cases/greet_numeric.yaml"
        } else {
            "cases/greet_pass.yaml"
        }))
        .arg("--oneharness-bin")
        .arg(boundary)
        .args(["--format", "json"])
        .env("JOURNEY_DIR", &scratch.0)
        .env("JUDGE_EXIT", exit.to_string())
        .output()
        .unwrap();
    (scratch, output)
}

#[test]
fn validated_structured_verdicts_precede_text_and_allow_empty_text() {
    for (numeric, value) in [(false, json!(true)), (true, json!(9))] {
        for text in [
            Value::Null,
            json!(""),
            json!("{\"value\":false,\"reason\":\"contradiction\"}"),
        ] {
            let (scratch, output) = journey(
                json!({"status":"ok","text":text,"structured":{"value":value,"reason":"validated"},"schema_valid":true,"usage":{"input_tokens":3,"output_tokens":2}}),
                numeric,
                0,
            );
            assert_eq!(
                output.status.code(),
                Some(0),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let report: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(report["runs"][0]["evals"][0]["reason"], "validated");
            let schema: Value =
                serde_json::from_slice(&std::fs::read(scratch.0.join("schema.json")).unwrap())
                    .unwrap();
            assert_eq!(
                schema["properties"]["value"]["type"],
                if numeric { "number" } else { "boolean" }
            );
            assert_eq!(schema["required"], json!(["value", "reason"]));
            let path = std::fs::read_to_string(scratch.0.join("schema-path")).unwrap();
            assert!(
                !std::path::Path::new(&path).exists(),
                "schema is cleaned up"
            );
            assert_eq!(
                report["runs"][0]["usage"]["input_tokens"],
                if numeric { 3 } else { 6 }
            );
        }
    }
}

#[test]
fn text_backstop_selects_last_complete_typed_verdict() {
    for metadata in [Value::Null, json!(false), json!(true)] {
        let (_, output) = journey(
            json!({"status":"ok","structured":{"unrelated":true},"schema_valid":metadata,"text":r#"{bad fragment {"value":false,"reason":"first"} Reconsidering {"other":"brace } and escaped \" quote"} {"value":true,"reason":"last { brace"} prose {"value":false"#}),
            false,
            if metadata == json!(false) { 1 } else { 0 },
        );
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["runs"][0]["evals"][0]["reason"], "last { brace");
    }
    let (_, output) = journey(
        json!({"status":"ok","text":"{\"value\":2} then {\"value\":8,\"reason\":\"revised\"} trailing prose"}),
        true,
        0,
    );
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn malformed_wrong_type_and_failed_status_remain_provider_errors() {
    for result in [
        json!({"status":"ok","text":"no verdict"}),
        json!({"status":"ok","text":"{\"value\":4}"}),
        json!({"status":"error","error":"auth denied","structured":{"value":true,"reason":"invalid status"},"schema_valid":true}),
    ] {
        let (scratch, output) = journey(result, false, 0);
        assert_eq!(
            output.status.code(),
            Some(3),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let path = std::fs::read_to_string(scratch.0.join("schema-path")).unwrap();
        assert!(!std::path::Path::new(&path).exists());
    }
}
