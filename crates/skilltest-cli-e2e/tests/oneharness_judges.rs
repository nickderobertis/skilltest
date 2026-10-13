//! Built-CLI journeys for oneharness reports, configuration layers, and literal argv transport.
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
        json!({"results": if result.is_array() {result} else {json!([result])}}).to_string(),
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

#[test]
fn ordered_layers_and_role_defaults_reach_the_provider_boundary() {
    let scratch = Scratch::new();
    let boundary = scratch.0.join("oneharness");
    let log = scratch.0.join("argv");
    std::fs::write(&boundary, r#"#!/bin/sh
printf '%s\n' CALL "$@" >> "$CONFIG_ARGV_LOG"
cat >/dev/null
printf '%s\n' '{"results":[{"status":"ok","text":"Hello confirmed","schema_valid":true,"structured":{"value":true,"reason":"passed"}}]}'
"#).unwrap();
    std::fs::set_permissions(&boundary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let config = scratch.0.join("config.json");
    std::fs::write(&config,json!({"provider":{"kind":"oneharness","bin":boundary,"judge_harness":"file-shorthand","timeout_secs":18,"judge":{"args":["--harness=file-role","--model=file-model","--timeout=29"]}},"judge_model":"file-model-shorthand"}).to_string()).unwrap();
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    for (sets, named, expected) in [
        (
            vec![],
            vec![],
            vec!["--harness=file-role", "--model=file-model", "--timeout=29"],
        ),
        (
            vec![
                "provider.judge_harness=\"set-harness\"",
                "judge_model=\"set-model\"",
                "provider.timeout_secs=31",
            ],
            vec![],
            vec!["set-harness", "set-model", "31"],
        ),
        (
            vec!["judge_model=\"earlier\"", "provider.judge={} "],
            vec![],
            vec!["--harness=file-role", "--model=file-model"],
        ),
        (
            vec!["provider.judge=null", "provider.judge={}"],
            vec![],
            vec![],
        ),
        (
            vec!["provider.judge.args=[\"--harness=role\",\"--model=role-model\",\"--timeout=7\"]"],
            vec![
                "--judge-harness",
                "named",
                "--judge-model",
                "named-model",
                "--timeout",
                "42",
            ],
            vec!["named", "named-model", "42"],
        ),
    ] {
        std::fs::write(&log, "").unwrap();
        let mut command = Command::new(common::built_bin("skilltest"));
        command
            .arg("--config")
            .arg(&config)
            .arg("run")
            .arg(fixtures.join("cases/greet_pass.yaml"))
            .args(["--format", "json"])
            .env("CONFIG_ARGV_LOG", &log);
        for assignment in &sets {
            command.args(["--set", assignment]);
        }
        command.args(&named);
        let output = command.output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(0),
            "{sets:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = std::fs::read_to_string(&log).unwrap();
        let judge = text.split("CALL\n").nth(2).unwrap();
        for value in expected {
            assert!(judge.lines().any(|line| line == value), "{sets:?}: {judge}");
        }
        if sets == ["provider.judge=null", "provider.judge={}"] {
            assert!(
                !judge.contains("--model")
                    && !judge.contains("--harness")
                    && !judge.contains("--timeout"),
                "explicit empty role uses defaults: {judge}"
            );
        }
        if !named.is_empty() {
            assert!(!judge.contains("=role") && !judge.contains("--timeout=7"));
        }
    }
    // The environment is one complete layer: a role block owns same-layer defaults.
    std::fs::write(&log, "").unwrap();
    let output = Command::new(common::built_bin("skilltest"))
        .arg("--config")
        .arg(&config)
        .arg("run")
        .arg(fixtures.join("cases/greet_pass.yaml"))
        .env("CONFIG_ARGV_LOG", &log)
        .env("SKILLTEST_PROVIDER__JUDGE", "null")
        .env("SKILLTEST_PROVIDER__JUDGE__ARGS", "[]")
        .env("SKILLTEST_JUDGE_MODEL", "env-model")
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let observations = std::fs::read_to_string(&log).unwrap();
    let judge = observations.split("CALL\n").nth(2).unwrap();
    assert!(
        !judge.contains("--model") && !judge.contains("env-model"),
        "{judge}"
    );
    // A later ordered assignment overrides that environment role.
    let output = Command::new(common::built_bin("skilltest"))
        .arg("--config")
        .arg(&config)
        .arg("run")
        .arg(fixtures.join("cases/greet_pass.yaml"))
        .env("CONFIG_ARGV_LOG", &log)
        .env("SKILLTEST_PROVIDER__JUDGE", "null")
        .env("SKILLTEST_PROVIDER__JUDGE__ARGS", "[]")
        .env("SKILLTEST_JUDGE_MODEL", "env-model")
        .args(["--set", "judge_model=\"later-model\""])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(std::fs::read_to_string(&log)
        .unwrap()
        .contains("later-model"));
    for (raw, expected) in [("123", "123"), ("null", "null"), ("\"null\"", "null")] {
        // A non-nullable shorthand rejects null; quoted null remains literal.
        let output = Command::new(common::built_bin("skilltest"))
            .arg("--config")
            .arg(&config)
            .arg("run")
            .arg(fixtures.join("cases/greet_pass.yaml"))
            .env("CONFIG_ARGV_LOG", &log)
            .env("SKILLTEST_JUDGE_MODEL", raw)
            .output()
            .unwrap();
        if raw == "null" {
            assert_eq!(output.status.code(), Some(2));
        } else {
            assert_eq!(
                output.status.code(),
                Some(0),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(std::fs::read_to_string(&log)
                .unwrap()
                .lines()
                .any(|line| line == expected));
        }
    }
    for setting in [
        "provider.judge.args.0=\"x\"",
        "spy=null",
        "max_turns=0",
        "models=[]",
        "judge_model=bad-json",
        "provider.judge.args=[\"--schema=x\"]",
        "provider.judge.args=[\"--run-mode=parallel\"]",
        "provider.judge.args=[\"--stream\"]",
    ] {
        let output = Command::new(common::built_bin("skilltest"))
            .arg("--config")
            .arg(&config)
            .arg("run")
            .arg(fixtures.join("cases/greet_pass.yaml"))
            .args(["--set", setting])
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(2),
            "{setting}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!output.stderr.is_empty());
    }
}

#[test]
fn fallback_reports_choose_settled_success_and_reject_multiple_verdicts() {
    let (_, output) = journey(
        json!([
            {"status":"error","failure_kind":"model_not_found","error":"candidate unavailable"},
            {"status":"ok","schema_valid":true,"structured":{"value":true,"reason":"settled"}}
        ]),
        false,
        0,
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let (_, output) = journey(
        json!([
            {"status":"ok","schema_valid":true,"structured":{"value":true,"reason":"first"}},
            {"status":"ok","schema_valid":true,"structured":{"value":false,"reason":"second"}}
        ]),
        false,
        0,
    );
    assert_eq!(output.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&output.stderr).contains("multiple successful results"));
    // Existing but unvalidated structured data cannot overrule the text backstop.
    for valid in [Value::Null, json!(false)] {
        let (_, output) = journey(
            json!({"status":"ok","schema_valid":valid,"structured":{"value":false,"reason":"unvalidated"},"text":"{\"value\":true,\"reason\":\"text verdict\"}"}),
            false,
            0,
        );
        assert_eq!(output.status.code(), Some(0));
    }
    let (_, output) = journey(
        json!({"status":"ok","schema_valid":true,"structured":{"value":false,"reason":"criterion failed"},"text":"{\"value\":true,\"reason\":\"contradiction\"}"}),
        false,
        0,
    );
    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["passed"], false);
}

#[test]
fn transport_failures_and_invalid_patch_contents_cannot_be_hidden() {
    let (_, output) = journey(
        json!({"status":"ok","schema_valid":true,"structured":{"value":true,"reason":"transport failed"}}),
        false,
        1,
    );
    assert_eq!(output.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&output.stderr).contains("exited with"));
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/cases/greet_pass.yaml");
    for assignment in [
        "provider={\"unknown\":1}",
        "mocks=[{\"match\":{\"tool\":2}}]",
        "provider.judge.args=[false]",
        "max_turns=-1",
        "max_turns=4294967296",
        "provider.judge.args=[\"--stream\"]",
        "mocks=[{\"match\":{\"tool\":\"x\"},\"stub\":[]}]",
    ] {
        let output = Command::new(common::built_bin("skilltest"))
            .arg("run")
            .arg(&fixture)
            .args([
                "--set",
                assignment,
                "--set",
                "provider={\"kind\":\"command\",\"command\":[\"unused\"]}",
                "--set",
                "max_turns=3",
                "--set",
                "mocks=[]",
            ])
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(2),
            "{assignment}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("config setting"));
    }
    let output = Command::new(common::built_bin("skilltest"))
        .args(["run", "--set", "max_turns"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("config assignment `max_turns` needs dotted.path=<JSON-value>"));
    for args in [
        json!(["--all"]),
        json!(["--prompt", "replacement"]),
        json!(["--prompt-file=replacement"]),
        json!(["--system", "replacement"]),
        json!(["--system-file=replacement"]),
        json!(["--format=json"]),
        json!(["--compact"]),
        json!(["--batch-strategy", "parallel"]),
        json!(["--batch-concurrency=2"]),
        json!(["--print-command"]),
        json!(["--schema=replacement"]),
        json!(["--run-mode=parallel"]),
    ] {
        let assignment = format!("provider.judge.args={args}");
        let output = Command::new(common::built_bin("skilltest"))
            .args(["run", "--set", &assignment])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{args}: {:?}", output);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("provider.judge.args"), "{stderr}");
        let flag = args[0].as_str().unwrap().split('=').next().unwrap();
        assert!(stderr.contains(flag), "{stderr}");
    }
    // Unrelated non-Unicode environment entries must not crash the resolver.
    use std::os::unix::ffi::OsStringExt;
    let output = Command::new(common::built_bin("skilltest"))
        .arg("run")
        .arg(fixture)
        .arg("--provider")
        .arg(common::built_bin("skilltest-fake-provider"))
        .env(
            "UNRELATED_BINARY_ENV",
            std::ffi::OsString::from_vec(vec![0xff]),
        )
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = Command::new(common::built_bin("skilltest"))
        .arg("run")
        .arg("--provider")
        .arg(common::built_bin("skilltest-fake-provider"))
        .env(
            "SKILLTEST_JUDGE_MODEL",
            std::ffi::OsString::from_vec(vec![0xff]),
        )
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("SKILLTEST_JUDGE_MODEL"));
}

#[test]
fn provider_variant_transitions_drop_old_fields_and_arrays_replace() {
    let scratch = Scratch::new();
    let config = scratch.0.join("config.json");
    let fake = common::built_bin("skilltest-fake-provider");
    std::fs::write(&config,json!({"provider":{"kind":"command","command":["unavailable"]},"platforms":["old-one","old-two"],"models":["old-one","old-two"]}).to_string()).unwrap();
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    let output = Command::new(common::built_bin("skilltest"))
        .arg("--config")
        .arg(&config)
        .arg("run")
        .arg(fixtures.join("cases/greet_pass.yaml"))
        .args([
            "--set",
            "provider={\"kind\":\"oneharness\",\"history\":false}",
        ])
        .arg("--set")
        .arg(format!(
            "provider={}",
            json!({"kind":"command","command":[fake]})
        ))
        .args([
            "--set",
            "platforms=[\"replacement\"]",
            "--set",
            "models=[\"replacement\"]",
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["runs"].as_array().unwrap().len(), 1);
    assert_eq!(report["runs"][0]["platform"], "replacement");
    assert_eq!(report["runs"][0]["model"], "replacement");
}

#[test]
fn generic_command_argv_preserves_spaces_empty_tokens_and_literal_shell_text() {
    let scratch = Scratch::new();
    let wrapper = scratch.0.join("provider with spaces");
    std::fs::write(&wrapper,r#"#!/bin/sh
if [ "$#" -ne 3 ] || [ "$1" != 'value with spaces' ] || [ "$2" != 'literal=$not-shell' ] || [ -n "$3" ]; then
  echo 'argv tokens were changed; preserve each JSON command array element' >&2
  exit 1
fi
exec "$CONFIG_FAKE_PROVIDER"
"#).unwrap();
    let provider = json!({"kind":"command","command":["/bin/sh",wrapper,"value with spaces","literal=$not-shell",""]});
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/cases/greet_pass.yaml");
    let output = Command::new(common::built_bin("skilltest"))
        .arg("run")
        .arg(fixture)
        .arg("--set")
        .arg(format!("provider={provider}"))
        .env(
            "CONFIG_FAKE_PROVIDER",
            common::built_bin("skilltest-fake-provider"),
        )
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
