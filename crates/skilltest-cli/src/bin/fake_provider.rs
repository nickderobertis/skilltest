//! `skilltest-fake-provider` — a deterministic reference implementation of the
//! provider protocol (see `docs/protocol.md`).
//!
//! It calls out to nothing: it reads one JSON request on stdin and writes one
//! JSON response on stdout, using simple, documented rules so the e2e suite can
//! drive the whole framework without a live model.
//!
//! Rules:
//!   * `respond` — replies with the text after a `fake-reply:` marker in the
//!     skill's instructions (or `"ok"` if absent). Never reports `done`.
//!   * `user` — replies with the text after a `say:` marker in the persona (or
//!     `"continue"`). Never stops on its own.
//!   * `judge` — scores against the concatenated assistant text. Backtick-quoted
//!     substrings in the criterion are required to appear; a `turns>=N` token is
//!     true once there have been N assistant turns. Boolean = all-present and
//!     turns-ok. Numeric = the fraction present, mapped onto `[min, max]` (0 if
//!     the turns gate fails).

use std::io::Read;

use serde_json::{json, Value};

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
    json!({ "message": reply, "done": false })
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
