//! Built-CLI journeys for what a judge is shown: a two-turn conversation whose
//! skill turns record tool events, judged by each judge backend. Only the model
//! is doubled — a fake `oneharness` for the skill, simulated user and
//! oneharness judge, and a fake `curl` for the API judge — and each double
//! records the prompt it was sent. The doubled judge decides from the recorded
//! tool output, never from the reply, which claims success either way, so a
//! verdict can only be right if the recorded activity reached the judge.
#![cfg(unix)]
mod common;
use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const PUSHED: &str = "Everything up-to-date";
const REJECTED: &str = "error: failed to push some refs";
/// What the doubled judge looks for: the second turn's recorded push output.
const EVIDENCE: &str = "output: Everything up-to-date";

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "skilltest-judge-tools-{}-{}",
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(dir.join("skill")).unwrap();
        Self(dir)
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.0.join(name)).unwrap()
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Each double's shell prelude: `verdict <request>` names the canned verdict
/// for a judge request — its kind from the prompt's wording, its value from
/// whether the recorded push output is in it.
fn executable(path: &Path, body: &str) {
    let prelude = format!(
        "d=\"$JOURNEY_DIR\"\n\
         verdict() {{\n\
         kind=boolean; grep -q 'Score how well' \"$1\" && kind=numeric\n\
         value=false; grep -qF '{EVIDENCE}' \"$1\" && value=true\n\
         echo \"$kind-$value\"\n\
         }}\n"
    );
    std::fs::write(path, format!("#!/bin/sh\n{prelude}{body}")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// 1,808 characters over the judge's 8,192-character output limit.
fn long_output() -> String {
    format!("{}{}", "a".repeat(8_192), "b".repeat(1_808))
}

/// Lay out the skill, the case and the doubles; `push_output` is what the
/// second turn's recorded `git push` returned.
fn conversation(push_output: &str) -> Scratch {
    let scratch = Scratch::new();
    let dir = &scratch.0;
    std::fs::write(
        dir.join("skill/SKILL.md"),
        "---\nname: tidier\ndescription: tidies a repo\n---\nTidy the repo.\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("case.yaml"),
        "name: pushes\nskill: skill\ninput: Tidy the repo\n\
         user:\n  persona: a busy maintainer\n  max_turns: 2\n\
         evals:\n  - type: boolean\n    criterion: the push reported the branch up to date\n  \
         - type: numeric\n    criterion: how completely the push succeeded\n    \
         min: 0\n    max: 10\n    threshold: 7\n",
    )
    .unwrap();
    let reply = |text: &str, events: Value| {
        json!({ "results": [{ "status": "ok", "text": text, "events": events }] }).to_string()
    };
    let write = |name: &str, body: String| std::fs::write(dir.join(name), body).unwrap();
    write(
        "respond-reply-0.json",
        reply(
            "I checked the log and the config.",
            json!([
                { "kind": "tool_call", "name": "bash",
                  "input": { "command": "cat big.log" }, "index": 0 },
                { "kind": "tool_result", "output": long_output(), "index": 1 },
                { "kind": "tool_call", "name": "read_file", "index": 2 },
            ]),
        ),
    );
    write(
        "respond-reply-1.json",
        reply(
            "Pushed.",
            json!([
                { "kind": "tool_call", "name": "bash",
                  "input": { "command": "git push" }, "index": 0 },
                { "kind": "tool_result", "name": "bash", "output": push_output, "index": 1 },
            ]),
        ),
    );
    write("user-reply-0.json", reply("Now push it.", Value::Null));
    for (name, value) in [
        ("boolean-true", json!(true)),
        ("boolean-false", json!(false)),
        ("numeric-true", json!(10)),
        ("numeric-false", json!(0)),
    ] {
        let verdict = json!({ "value": value, "reason": "from the recorded push output" });
        write(
            &format!("oh-{name}.json"),
            json!({ "results": [{ "status": "ok", "text": verdict.to_string() }] }).to_string(),
        );
        write(
            &format!("api-{name}.json"),
            json!({ "content": [{ "type": "text", "text": verdict.to_string() }] }).to_string(),
        );
    }
    write(
        "api-user.json",
        json!({ "content": [{ "type": "text", "text": "Now push it." }] }).to_string(),
    );
    // llmlint: ignore-block[e2e_not_mocked] These two executables are the models — skill, simulated user and both judge backends — doubled at the paid model boundary and nowhere above: everything between them and the case file is the real built `skilltest` (config, runner, both judge backends, prompt building, verdict parsing). AGENTS.md: "No non-determinism in the gate: the LLM is always faked in tests"; the live tiers (`live/`) drive real harnesses and APIs.
    executable(
        &dir.join("oneharness"),
        "case \" $* \" in\n\
         *' --schema '*) role=judge ;;\n\
         *' --system '*) role=respond ;;\n\
         *) role=user ;;\n\
         esac\n\
         n=$(ls \"$d\" | grep -c \"^$role-prompt-\")\n\
         cat > \"$d/$role-prompt-$n.txt\"\n\
         if [ \"$role\" = judge ]; then\n\
         cat \"$d/oh-$(verdict \"$d/judge-prompt-$n.txt\").json\"\n\
         else\n\
         cat \"$d/$role-reply-$n.json\"\n\
         fi\n",
    );
    executable(
        &dir.join("curl"),
        "n=$(ls \"$d\" | grep -c '^curl-body-')\n\
         cat > \"$d/curl-body-$n.json\"\n\
         if grep -q '\"output_config\"' \"$d/curl-body-$n.json\"; then\n\
         cat \"$d/api-$(verdict \"$d/curl-body-$n.json\").json\"\n\
         else\n\
         cat \"$d/api-user.json\"\n\
         fi\n",
    );
    // llmlint: ignore-end[e2e_not_mocked]
    std::fs::write(
        dir.join("api-judge.yaml"),
        format!(
            "judge:\n  kind: api\n  vendor: anthropic\n  api_key_env: SKILLTEST_E2E_JUDGE_KEY\n  \
             base_url: https://example.invalid/v1\n  curl_bin: {}\n",
            dir.join("curl").display()
        ),
    )
    .unwrap();
    scratch
}

fn run(scratch: &Scratch, api_judge: bool) -> (Output, Value) {
    let mut command = Command::new(common::built_bin("skilltest"));
    if api_judge {
        command
            .arg("--config")
            .arg(scratch.0.join("api-judge.yaml"));
    }
    let output = command
        .arg("run")
        .arg(scratch.0.join("case.yaml"))
        .arg("--oneharness-bin")
        .arg(scratch.0.join("oneharness"))
        .args(["--format", "json"])
        .env("JOURNEY_DIR", &scratch.0)
        .env("SKILLTEST_E2E_JUDGE_KEY", "sk-test")
        .output()
        .unwrap();
    let report = serde_json::from_slice(&output.stdout).unwrap_or_else(|err| {
        panic!(
            "{err}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output, report)
}

/// Every prompt the judge and simulated-user doubles were sent, as
/// `(judge, user)`. The API backend carries both roles, telling a judge request
/// by its schema-constrained `output_config`.
fn prompts(scratch: &Scratch, api_judge: bool) -> (Vec<String>, Vec<String>) {
    let read_all = |prefix: &str| -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(&scratch.0)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with(prefix))
            .collect();
        names.sort();
        names.iter().map(|name| scratch.read(name)).collect()
    };
    if !api_judge {
        return (read_all("judge-prompt-"), read_all("user-prompt-"));
    }
    let (mut judge, mut user) = (Vec::new(), Vec::new());
    for raw in read_all("curl-body-") {
        let body: Value = serde_json::from_str(raw.trim()).unwrap();
        let prompt = body["messages"][0]["content"].as_str().unwrap().to_string();
        if body.get("output_config").is_some() {
            judge.push(prompt);
        } else {
            user.push(prompt);
        }
    }
    (judge, user)
}

/// The transcript every judge backend must see: each turn's reply text with
/// that turn's recorded events ahead of it, in order, with kind, name, input
/// and output; the long output cut to 8,192 characters with the omitted count;
/// fields the recording lacks marked unavailable.
fn expected_transcript(push_output: &str) -> String {
    format!(
        "User: Tidy the repo\n\
         [Assistant tool activity: 3 recorded event(s), in order, as the harness recorded them]\n\
         --- tool event 1 ---\n\
         kind: tool_call\n\
         name: bash\n\
         input: {{\"command\":\"cat big.log\"}}\n\
         output: (unavailable: not recorded)\n\
         --- tool event 2 ---\n\
         kind: tool_result\n\
         name: (unavailable: not recorded)\n\
         input: (unavailable: not recorded)\n\
         output: {kept}\n\
         [output truncated: 1808 of 10000 characters omitted]\n\
         --- tool event 3 ---\n\
         kind: tool_call\n\
         name: read_file\n\
         input: (unavailable: not recorded)\n\
         output: (unavailable: not recorded)\n\
         [end of tool activity]\n\
         Assistant: I checked the log and the config.\n\
         User: Now push it.\n\
         [Assistant tool activity: 2 recorded event(s), in order, as the harness recorded them]\n\
         --- tool event 1 ---\n\
         kind: tool_call\n\
         name: bash\n\
         input: {{\"command\":\"git push\"}}\n\
         output: (unavailable: not recorded)\n\
         --- tool event 2 ---\n\
         kind: tool_result\n\
         name: bash\n\
         input: (unavailable: not recorded)\n\
         output: {push_output}\n\
         [end of tool activity]\n\
         Assistant: Pushed.",
        kept = "a".repeat(8_192),
    )
}

fn assert_journey(api_judge: bool, push_output: &str, verdict: bool) {
    let scratch = conversation(push_output);
    let (output, report) = run(&scratch, api_judge);
    assert_eq!(
        output.status.code(),
        Some(if verdict { 0 } else { 1 }),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let case_run = &report["runs"][0];
    assert_eq!(case_run["turns"], 2, "{report}");
    assert_eq!(case_run["evals"][0]["passed"], verdict, "{report}");
    assert_eq!(case_run["evals"][1]["passed"], verdict, "{report}");
    assert_eq!(
        case_run["transcript"]["messages"][3]["events"][1]["output"],
        push_output
    );

    let (judge, user) = prompts(&scratch, api_judge);
    assert_eq!((judge.len(), user.len()), (2, 1));
    for criterion in [
        "the push reported the branch up to date",
        "how completely the push succeeded",
    ] {
        let expected = format!(
            "Criterion: {criterion}\n\nTranscript:\n{}\n\n",
            expected_transcript(push_output)
        );
        let prompt = judge
            .iter()
            .find(|prompt| prompt.contains(&expected))
            .unwrap_or_else(|| panic!("no judge prompt carries {criterion:?}: {judge:#?}"));
        assert!(!prompt.contains(&"a".repeat(8_193)), "output was not cut");
    }

    // The simulated user and the inlined second skill turn keep the prose
    // rendering: reply text only, no tool activity.
    for prose in [&user[0], &scratch.read("respond-prompt-1.txt")] {
        assert!(
            prose.contains("User: Tidy the repo\nAssistant: I checked the log and the config.\n"),
            "{prose}"
        );
        assert!(!prose.contains("tool activity"), "{prose}");
    }
}

#[test]
fn oneharness_judge_decides_from_the_recorded_tool_output() {
    assert_journey(false, PUSHED, true);
}

#[test]
fn oneharness_judge_fails_a_reply_its_recorded_tool_output_contradicts() {
    assert_journey(false, REJECTED, false);
}

#[test]
fn api_judge_decides_from_the_recorded_tool_output() {
    assert_journey(true, PUSHED, true);
}

#[test]
fn api_judge_fails_a_reply_its_recorded_tool_output_contradicts() {
    assert_journey(true, REJECTED, false);
}
