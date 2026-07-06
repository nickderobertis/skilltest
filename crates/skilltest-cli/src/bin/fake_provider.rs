//! `skilltest-fake-provider` — a deterministic reference implementation of the
//! provider protocol (see `docs/protocol.md`).
//!
//! It calls out to nothing: it reads one JSON request on stdin and writes one
//! JSON response on stdout, using simple, documented rules so the e2e suite can
//! drive the whole framework without a live model.
//!
//! Rules:
//!   * `respond` — replies with the text after a `fake-reply:` marker in the
//!     skill's instructions (or `"ok"` if absent). Never reports `done`. Each
//!     `fake-tool: <name> <command>` marker becomes a normalized `tool_call`
//!     event on the turn (`name` = first token, `input.command` = the rest), so
//!     the e2e suite can exercise the tool-events path deterministically.
//!     When the request carries a `mocks` block, its compiled ruleset is
//!     applied to each scripted call with the same first-match semantics the
//!     oneharness hook uses (`skilltest_core::mock::decide` — one shared
//!     decision engine, so the gate proves the real logic): the response gains
//!     `mock_calls` records (original inputs + verdicts), intercepted events
//!     show post-rewrite reality (a stub's `printf`, a rewrite's input), and a
//!     stub's canned output / a deny's message is appended to the reply text as
//!     `[<tool>] <text>` — the deterministic stand-in for "the model saw the
//!     mocked result".
//!   * `user` — replies with the text after a `say:` marker in the persona (or
//!     `"continue"`). Never stops on its own.
//!   * `judge` — scores against the concatenated assistant text. Backtick-quoted
//!     substrings in the criterion are required to appear; a `turns>=N` token is
//!     true once there have been N assistant turns. Boolean = all-present and
//!     turns-ok. Numeric = the fraction present, mapped onto `[min, max]` (0 if
//!     the turns gate fails).

use std::io::Read;

use serde_json::{json, Value};
use skilltest_core::mock::{decide, stub_command, AppliedAction};

fn main() {
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        emit_error("could not read request from stdin");
    }
    let request: Value = match serde_json::from_str(input.trim()) {
        Ok(value) => value,
        Err(err) => emit_error(&format!("request was not valid JSON: {err}")),
    };

    let op = request.get("op").and_then(Value::as_str).unwrap_or("");
    let response = match op {
        "respond" => respond(&request),
        "user" => user(&request),
        "judge" => judge(&request),
        other => emit_error(&format!("unknown op `{other}`")),
    };

    println!("{response}");
}

fn respond(request: &Value) -> Value {
    let instructions = request
        .get("skill")
        .and_then(|s| s.get("instructions"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let reply = marker(instructions, "fake-reply:").unwrap_or_else(|| "ok".to_string());
    let calls = tool_calls(instructions);
    let Some(mocks) = request.get("mocks") else {
        // No mock channel requested: the pre-mock shape, no `mock_calls`.
        let events = plain_events(&calls);
        if events.is_empty() {
            return json!({ "message": reply, "done": false });
        }
        return json!({ "message": reply, "done": false, "events": events });
    };
    let rules = mocks.get("rules").filter(|r| !r.is_null());

    let mut events = Vec::new();
    let mut records = Vec::new();
    let mut surfaced: Vec<String> = Vec::new();
    for (index, (name, command)) in calls.iter().enumerate() {
        let input = json!({ "command": command });
        let decision = rules.and_then(|r| decide(r, Some(name), Some(&input)));
        let record_action = decision
            .as_ref()
            .map_or("allow", |(_, action)| action.kind());
        records.push(json!({
            "tool": name,
            "input": input,
            "action": record_action,
            "rule": decision.as_ref().map(|(rule, _)| rule),
        }));
        match decision {
            None => events.push(event(index, name, json!({ "command": command }), None)),
            Some((_, AppliedAction::Deny { message })) => {
                // The call never ran; the model reads the message as the
                // tool's feedback.
                events.push(event(
                    index,
                    name,
                    json!({ "command": command }),
                    Some(format!("denied: {message}")),
                ));
                surfaced.push(format!("[{name}] denied: {message}"));
            }
            Some((_, AppliedAction::Stub { output, exit_code })) => {
                // Post-rewrite reality: the printf stub "ran" and the model
                // received the canned output as the tool's genuine result.
                events.push(event(
                    index,
                    name,
                    json!({ "command": stub_command(output, exit_code) }),
                    Some(format!("{output}\n")),
                ));
                surfaced.push(format!("[{name}] {output}"));
            }
            Some((_, AppliedAction::Rewrite { input })) => {
                events.push(event(index, name, input.clone(), None));
                surfaced.push(format!("[{name}] input rewritten to {input}"));
            }
        }
    }
    // Intercepted results surface in the reply text, standing in for "the
    // model relayed what the mocked tool returned" — so evals can assert the
    // canned output reached the conversation.
    let message = if surfaced.is_empty() {
        reply
    } else {
        format!("{reply}\n{}", surfaced.join("\n"))
    };
    if events.is_empty() {
        json!({ "message": message, "done": false, "mock_calls": records })
    } else {
        json!({ "message": message, "done": false, "events": events, "mock_calls": records })
    }
}

/// One normalized `tool_call` event.
fn event(index: usize, name: &str, input: Value, output: Option<String>) -> Value {
    match output {
        Some(output) => json!({
            "kind": "tool_call",
            "name": name,
            "input": input,
            "output": output,
            "index": index,
        }),
        None => json!({
            "kind": "tool_call",
            "name": name,
            "input": input,
            "index": index,
        }),
    }
}

/// The unmocked events shape (no `mocks` block on the request).
fn plain_events(calls: &[(String, String)]) -> Vec<Value> {
    calls
        .iter()
        .enumerate()
        .map(|(index, (name, command))| event(index, name, json!({ "command": command }), None))
        .collect()
}

/// Parse the instructions' tool markers into scripted `(name, command)` calls:
///   * `fake-tool: <name> <command>` — one call (`name` = first token).
///   * `fake-tools: <N>` — N generic `noop` calls, for exercising streaming
///     backpressure (the CLI short-circuit path) without a huge fixture.
fn tool_calls(instructions: &str) -> Vec<(String, String)> {
    let mut calls: Vec<(String, String)> = Vec::new();
    for line in instructions.lines() {
        if let Some(idx) = line.find("fake-tools:") {
            let rest = line[idx + "fake-tools:".len()..]
                .trim()
                .trim_end_matches("-->")
                .trim();
            if let Ok(count) = rest.parse::<usize>() {
                calls.extend((0..count).map(|_| ("noop".to_string(), String::new())));
            }
        } else if let Some(idx) = line.find("fake-tool:") {
            let rest = line[idx + "fake-tool:".len()..]
                .trim()
                .trim_end_matches("-->")
                .trim();
            let (name, command) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            calls.push((name.to_string(), command.trim().to_string()));
        }
    }
    calls
}

fn user(request: &Value) -> Value {
    let persona = request.get("persona").and_then(Value::as_str).unwrap_or("");
    let reply = marker(persona, "say:").unwrap_or_else(|| "continue".to_string());
    json!({ "message": reply, "stop": false })
}

fn judge(request: &Value) -> Value {
    let kind = request
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("boolean");
    let criterion = request
        .get("criterion")
        .and_then(Value::as_str)
        .unwrap_or("");
    let messages = request
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let assistant_text = assistant_text(&messages);
    let assistant_turns = assistant_turns(&messages);

    let required = backtick_substrings(criterion);
    let present = required
        .iter()
        .filter(|needle| assistant_text.contains(needle.as_str()))
        .count();
    let turns_ok = match required_turns(criterion) {
        Some(n) => assistant_turns >= n,
        None => true,
    };

    match kind {
        "numeric" => {
            let min = request.get("min").and_then(Value::as_f64).unwrap_or(0.0);
            let max = request.get("max").and_then(Value::as_f64).unwrap_or(1.0);
            let fraction = if required.is_empty() {
                1.0
            } else {
                present as f64 / required.len() as f64
            };
            let base = if turns_ok { fraction } else { 0.0 };
            let value = min + (max - min) * base;
            json!({
                "value": value,
                "reason": format!(
                    "{present}/{} required substrings present, turns_ok={turns_ok}",
                    required.len()
                ),
            })
        }
        _ => {
            let value = present == required.len() && turns_ok;
            json!({
                "value": value,
                "reason": format!(
                    "{present}/{} required substrings present, turns_ok={turns_ok}",
                    required.len()
                ),
            })
        }
    }
}

/// Concatenate the content of every assistant message.
fn assistant_text(messages: &[Value]) -> String {
    messages
        .iter()
        .filter(|m| m.get("role").and_then(Value::as_str) == Some("assistant"))
        .filter_map(|m| m.get("content").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
}

fn assistant_turns(messages: &[Value]) -> usize {
    messages
        .iter()
        .filter(|m| m.get("role").and_then(Value::as_str) == Some("assistant"))
        .count()
}

/// Extract the text after a `marker` on its line, stripping a trailing `-->`
/// (so the marker can live inside an HTML comment in a SKILL.md body).
fn marker(text: &str, marker: &str) -> Option<String> {
    text.lines().find_map(|line| {
        line.find(marker).map(|idx| {
            line[idx + marker.len()..]
                .trim()
                .trim_end_matches("-->")
                .trim()
                .to_string()
        })
    })
}

/// All substrings wrapped in backticks within `text`.
fn backtick_substrings(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = text.char_indices();
    while let Some((start, ch)) = chars.next() {
        if ch == '`' {
            let rest = &text[start + 1..];
            if let Some(end) = rest.find('`') {
                let needle = &rest[..end];
                if !needle.is_empty() {
                    out.push(needle.to_string());
                }
                // Advance the iterator past the closing backtick.
                for _ in 0..=end {
                    chars.next();
                }
            }
        }
    }
    out
}

/// Parse a `turns>=N` token from the criterion, if present.
fn required_turns(criterion: &str) -> Option<usize> {
    let idx = criterion.find("turns>=")?;
    let digits: String = criterion[idx + "turns>=".len()..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

/// Emit a protocol-level error to stderr and exit non-zero, which the core
/// surfaces as a provider error.
fn emit_error(message: &str) -> ! {
    eprintln!("fake-provider: {message}");
    std::process::exit(1);
}
