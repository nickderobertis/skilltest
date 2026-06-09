//! The provider boundary. `skilltest` never talks to a model directly; a
//! [`Provider`] runs the skill, plays the simulated user, and judges the
//! transcript.
//!
//! There are two real implementations. [`OneharnessProvider`] (the default) runs
//! each prompt on a harness through the
//! [`oneharness`](https://github.com/nickderobertis/oneharness) CLI and parses
//! its JSON. [`CommandProvider`] speaks a small JSON-lines protocol (see
//! `docs/protocol.md`) and backs both the deterministic `skilltest-fake-provider`
//! used by the gate and any custom provider you write. The [`Provider`] trait
//! also lets the runner be unit-tested against an in-memory fake.

use std::io::Write as _;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::config::OneharnessConfig;
use crate::conversation::{Message, Role};
use crate::error::{Error, Result};
use crate::eval::JudgeValue;

/// A borrowed view of the skill under test, as sent to the provider.
pub struct SkillRef<'a> {
    pub name: &'a str,
    pub dir: &'a str,
    pub instructions: &'a str,
}

/// The kind of judgement requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JudgeKind {
    Boolean,
    Numeric,
}

impl JudgeKind {
    fn as_str(self) -> &'static str {
        match self {
            JudgeKind::Boolean => "boolean",
            JudgeKind::Numeric => "numeric",
        }
    }
}

/// A judge query: the criterion, its kind, and (for numeric) the scale.
pub struct JudgeQuery<'a> {
    pub kind: JudgeKind,
    pub criterion: &'a str,
    pub scale: Option<(f64, f64)>,
}

/// An assistant/skill turn produced by the provider.
#[derive(Debug, Clone)]
pub struct AssistantTurn {
    pub message: String,
    /// The skill signalled it considers the task complete.
    pub done: bool,
}

/// A simulated-user turn produced by the provider.
#[derive(Debug, Clone)]
pub struct UserTurn {
    pub message: String,
    /// The simulated user chose to end the conversation.
    pub stop: bool,
}

/// A judge verdict: the raw value (bool or number) plus the stated reason.
#[derive(Debug, Clone)]
pub struct JudgeVerdict {
    pub value: JudgeValue,
    pub reason: String,
}

/// The provider boundary.
pub trait Provider {
    /// Run one assistant/skill turn given the conversation so far.
    ///
    /// # Errors
    /// [`Error::Provider`] if the command fails or returns malformed output.
    fn respond(
        &self,
        platform: &str,
        model: &str,
        skill: &SkillRef<'_>,
        messages: &[Message],
    ) -> Result<AssistantTurn>;

    /// Produce one simulated-user turn.
    ///
    /// # Errors
    /// [`Error::Provider`] if the command fails or returns malformed output.
    fn simulate_user(&self, model: &str, persona: &str, messages: &[Message]) -> Result<UserTurn>;

    /// Score a criterion against the conversation.
    ///
    /// # Errors
    /// [`Error::Provider`] if the command fails or returns malformed output.
    fn judge(
        &self,
        model: &str,
        query: &JudgeQuery<'_>,
        messages: &[Message],
    ) -> Result<JudgeVerdict>;
}

// ---------------------------------------------------------------------------
// Wire types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct SkillPayload<'a> {
    name: &'a str,
    path: &'a str,
    instructions: &'a str,
}

#[derive(Serialize)]
#[serde(tag = "op", rename_all = "lowercase")]
enum Request<'a> {
    Respond {
        platform: &'a str,
        model: &'a str,
        skill: SkillPayload<'a>,
        messages: &'a [Message],
    },
    User {
        model: &'a str,
        persona: &'a str,
        messages: &'a [Message],
    },
    Judge {
        model: &'a str,
        kind: &'a str,
        criterion: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        min: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        max: Option<f64>,
        messages: &'a [Message],
    },
}

#[derive(Deserialize)]
struct RespondPayload {
    message: String,
    #[serde(default)]
    done: bool,
}

#[derive(Deserialize)]
struct UserPayload {
    message: String,
    #[serde(default)]
    stop: bool,
}

#[derive(Deserialize)]
struct JudgePayload {
    value: JudgeValue,
    #[serde(default)]
    reason: String,
}

// ---------------------------------------------------------------------------
// CommandProvider
// ---------------------------------------------------------------------------

/// A [`Provider`] backed by an external command speaking the JSON protocol.
pub struct CommandProvider {
    argv: Vec<String>,
}

impl CommandProvider {
    /// Build a provider from an argv vector (program + args). The program is
    /// resolved on `PATH`.
    ///
    /// # Errors
    /// [`Error::Invalid`] if `argv` is empty.
    pub fn new(argv: Vec<String>) -> Result<Self> {
        if argv.is_empty() {
            return Err(Error::Invalid("provider command is empty".into()));
        }
        Ok(Self { argv })
    }

    /// Send one request and parse the single response object from stdout.
    fn call<T: for<'de> Deserialize<'de>>(&self, request: &Request<'_>, op: &str) -> Result<T> {
        let payload = serde_json::to_vec(request).map_err(|e| {
            Error::provider(op.to_string(), format!("could not encode request: {e}"))
        })?;

        let mut child = Command::new(&self.argv[0])
            .args(&self.argv[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                Error::provider(
                    op.to_string(),
                    format!(
                        "could not run provider `{}`: {e}. Is it installed and on PATH?",
                        self.argv[0]
                    ),
                )
            })?;

        // Write the request, then close stdin so the child can finish. Writing
        // before reading stdout is safe here because responses are small.
        {
            let stdin = child
                .stdin
                .as_mut()
                .ok_or_else(|| Error::provider(op.to_string(), "could not open provider stdin"))?;
            stdin
                .write_all(&payload)
                .and_then(|()| stdin.write_all(b"\n"))
                .map_err(|e| {
                    Error::provider(op.to_string(), format!("could not write request: {e}"))
                })?;
        }

        let output = child.wait_with_output().map_err(|e| {
            Error::provider(op.to_string(), format!("provider did not complete: {e}"))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::provider(
                op.to_string(),
                format!("provider exited with {}: {}", output.status, stderr.trim()),
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let line = stdout.trim();
        if line.is_empty() {
            return Err(Error::provider(
                op.to_string(),
                "provider produced no output (expected one JSON response object)",
            ));
        }
        serde_json::from_str(line).map_err(|e| {
            Error::provider(
                op.to_string(),
                format!("provider response was not valid JSON for `{op}`: {e}; got: {line}"),
            )
        })
    }
}

impl Provider for CommandProvider {
    fn respond(
        &self,
        platform: &str,
        model: &str,
        skill: &SkillRef<'_>,
        messages: &[Message],
    ) -> Result<AssistantTurn> {
        let request = Request::Respond {
            platform,
            model,
            skill: SkillPayload {
                name: skill.name,
                path: skill.dir,
                instructions: skill.instructions,
            },
            messages,
        };
        let payload: RespondPayload = self.call(&request, "respond")?;
        Ok(AssistantTurn {
            message: payload.message,
            done: payload.done,
        })
    }

    fn simulate_user(&self, model: &str, persona: &str, messages: &[Message]) -> Result<UserTurn> {
        let request = Request::User {
            model,
            persona,
            messages,
        };
        let payload: UserPayload = self.call(&request, "user")?;
        Ok(UserTurn {
            message: payload.message,
            stop: payload.stop,
        })
    }

    fn judge(
        &self,
        model: &str,
        query: &JudgeQuery<'_>,
        messages: &[Message],
    ) -> Result<JudgeVerdict> {
        let (min, max) = match query.scale {
            Some((lo, hi)) => (Some(lo), Some(hi)),
            None => (None, None),
        };
        let request = Request::Judge {
            model,
            kind: query.kind.as_str(),
            criterion: query.criterion,
            min,
            max,
            messages,
        };
        let payload: JudgePayload = self.call(&request, "judge")?;
        Ok(JudgeVerdict {
            value: payload.value,
            reason: payload.reason,
        })
    }
}

// ---------------------------------------------------------------------------
// OneharnessProvider
// ---------------------------------------------------------------------------

/// The default [`Provider`]: runs each prompt on a harness through the
/// `oneharness` CLI.
///
/// oneharness has no skill/judge/user/session concept — it is a stateless
/// prompt→text runner (`oneharness run --harness H --model M --prompt-file -`).
/// So this provider *builds the prompts*: it inlines the skill instructions and
/// the conversation for an assistant turn, frames a persona for a user turn, and
/// asks for a strict JSON verdict for a judge. Evals and the simulated user run
/// on a fixed `judge_harness`, independent of the harness under test.
pub struct OneharnessProvider {
    bin: String,
    judge_harness: String,
    timeout_secs: u64,
}

/// The subset of the `oneharness run` JSON envelope we consume.
#[derive(Deserialize)]
struct OhEnvelope {
    results: Vec<OhResult>,
}

#[derive(Deserialize)]
struct OhResult {
    status: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    stderr: String,
    #[serde(default)]
    error: Option<String>,
}

impl OneharnessProvider {
    /// Build a provider from its configuration.
    #[must_use]
    pub fn new(config: &OneharnessConfig) -> Self {
        Self {
            bin: config.bin.clone(),
            judge_harness: config.judge_harness.clone(),
            timeout_secs: config.timeout_secs,
        }
    }

    /// Run one prompt on `harness` with `model` and return the normalized text.
    fn run(&self, harness: &str, model: &str, prompt: &str) -> Result<String> {
        let timeout = self.timeout_secs.to_string();
        let mut child = Command::new(&self.bin)
            .args([
                "run",
                "--harness",
                harness,
                "--model",
                model,
                "--output-format",
                "json",
                "--compact",
                "--timeout",
                &timeout,
                "--prompt-file",
                "-",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                Error::provider(
                    "oneharness",
                    format!(
                        "could not run `{}`: {e}. Is oneharness installed and on PATH?",
                        self.bin
                    ),
                )
            })?;

        child
            .stdin
            .as_mut()
            .ok_or_else(|| Error::provider("oneharness", "could not open oneharness stdin"))?
            .write_all(prompt.as_bytes())
            .map_err(|e| Error::provider("oneharness", format!("could not write prompt: {e}")))?;

        let output = child.wait_with_output().map_err(|e| {
            Error::provider("oneharness", format!("oneharness did not complete: {e}"))
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let envelope: OhEnvelope = serde_json::from_str(stdout.trim()).map_err(|e| {
            Error::provider(
                "oneharness",
                format!(
                    "could not parse oneharness output: {e}; stderr: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            )
        })?;

        let result = envelope
            .results
            .into_iter()
            .next()
            .ok_or_else(|| Error::provider("oneharness", "oneharness returned no results"))?;

        if result.status != "ok" {
            let detail = result
                .error
                .filter(|e| !e.is_empty())
                .or_else(|| Some(result.stderr.clone()).filter(|s| !s.is_empty()))
                .unwrap_or_else(|| format!("status `{}`", result.status));
            return Err(Error::provider(
                format!("oneharness:{harness}"),
                format!("harness run failed: {detail}"),
            ));
        }

        result.text.filter(|t| !t.trim().is_empty()).ok_or_else(|| {
            Error::provider(
                format!("oneharness:{harness}"),
                "harness produced no extractable text",
            )
        })
    }
}

impl Provider for OneharnessProvider {
    fn respond(
        &self,
        platform: &str,
        model: &str,
        skill: &SkillRef<'_>,
        messages: &[Message],
    ) -> Result<AssistantTurn> {
        let prompt = build_respond_prompt(skill, messages);
        let text = self.run(platform, model, &prompt)?;
        Ok(AssistantTurn {
            message: text.trim().to_string(),
            done: false,
        })
    }

    fn simulate_user(&self, model: &str, persona: &str, messages: &[Message]) -> Result<UserTurn> {
        let prompt = build_user_prompt(persona, messages);
        let text = self.run(&self.judge_harness, model, &prompt)?;
        Ok(UserTurn {
            message: text.trim().to_string(),
            stop: false,
        })
    }

    fn judge(
        &self,
        model: &str,
        query: &JudgeQuery<'_>,
        messages: &[Message],
    ) -> Result<JudgeVerdict> {
        let prompt = build_judge_prompt(query, messages);
        let text = self.run(&self.judge_harness, model, &prompt)?;
        parse_verdict(query.kind, &text)
    }
}

/// Render the conversation as `Role: content` lines for inlining in a prompt.
fn render_transcript(messages: &[Message]) -> String {
    messages
        .iter()
        .map(|m| {
            let role = match m.role {
                Role::User => "User",
                Role::Assistant => "Assistant",
                Role::System => "System",
            };
            format!("{role}: {}", m.content)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn build_respond_prompt(skill: &SkillRef<'_>, messages: &[Message]) -> String {
    format!(
        "You are an assistant operating under the following skill instructions.\n\n\
         --- SKILL: {name} ---\n{instructions}\n--- END SKILL ---\n\n\
         Conversation so far (most recent last):\n{transcript}\n\n\
         Write only the assistant's next reply, following the skill. Output the \
         reply text and nothing else.",
        name = skill.name,
        instructions = skill.instructions,
        transcript = render_transcript(messages),
    )
}

fn build_user_prompt(persona: &str, messages: &[Message]) -> String {
    format!(
        "You are role-playing the USER in a conversation with an AI assistant. \
         Stay in character:\n\n{persona}\n\n\
         Conversation so far (most recent last):\n{transcript}\n\n\
         Write only the user's next message. Output the message text and nothing \
         else.",
        transcript = render_transcript(messages),
    )
}

fn build_judge_prompt(query: &JudgeQuery<'_>, messages: &[Message]) -> String {
    let transcript = render_transcript(messages);
    match query.kind {
        JudgeKind::Boolean => format!(
            "You are a strict, careful evaluator of an AI assistant's behavior.\n\n\
             Criterion: {criterion}\n\n\
             Transcript:\n{transcript}\n\n\
             Decide whether the criterion is satisfied. Respond with ONLY a \
             single-line JSON object and nothing else:\n\
             {{\"value\": true or false, \"reason\": \"<one short sentence>\"}}",
            criterion = query.criterion,
        ),
        JudgeKind::Numeric => {
            let (min, max) = query.scale.unwrap_or((0.0, 10.0));
            format!(
                "You are a strict, careful evaluator of an AI assistant's behavior.\n\n\
                 Criterion: {criterion}\n\n\
                 Transcript:\n{transcript}\n\n\
                 Score how well the criterion is satisfied on a scale from {min} to \
                 {max} (inclusive). Respond with ONLY a single-line JSON object and \
                 nothing else:\n\
                 {{\"value\": <number between {min} and {max}>, \"reason\": \"<one short sentence>\"}}",
                criterion = query.criterion,
            )
        }
    }
}

/// Extract the first JSON object from `text`, tolerating code fences and prose
/// around it (real models do not always emit bare JSON).
fn extract_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end > start {
        Some(&text[start..=end])
    } else {
        None
    }
}

fn parse_verdict(kind: JudgeKind, text: &str) -> Result<JudgeVerdict> {
    let json = extract_json_object(text).ok_or_else(|| {
        Error::provider(
            "oneharness:judge",
            format!("judge did not return a JSON object; got: {text}"),
        )
    })?;
    let value: serde_json::Value = serde_json::from_str(json).map_err(|e| {
        Error::provider(
            "oneharness:judge",
            format!("judge verdict was not valid JSON: {e}; got: {json}"),
        )
    })?;
    let reason = value
        .get("reason")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string();
    let raw = value
        .get("value")
        .ok_or_else(|| Error::provider("oneharness:judge", "judge verdict has no `value` field"))?;

    let verdict_value = match kind {
        JudgeKind::Boolean => JudgeValue::Bool(raw.as_bool().ok_or_else(|| {
            Error::provider(
                "oneharness:judge",
                format!("boolean judge `value` was not a bool: {raw}"),
            )
        })?),
        JudgeKind::Numeric => JudgeValue::Number(raw.as_f64().ok_or_else(|| {
            Error::provider(
                "oneharness:judge",
                format!("numeric judge `value` was not a number: {raw}"),
            )
        })?),
    };

    Ok(JudgeVerdict {
        value: verdict_value,
        reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_argv_is_rejected() {
        assert!(CommandProvider::new(vec![]).is_err());
    }

    #[test]
    fn request_serializes_with_op_tag() {
        let req = Request::Judge {
            model: "m",
            kind: "numeric",
            criterion: "polite",
            min: Some(0.0),
            max: Some(10.0),
            messages: &[],
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"op\":\"judge\""));
        assert!(json.contains("\"kind\":\"numeric\""));
    }

    #[test]
    fn respond_prompt_inlines_skill_and_transcript() {
        let skill = SkillRef {
            name: "greeter",
            dir: "/x",
            instructions: "Greet by name.",
        };
        let prompt = build_respond_prompt(&skill, &[Message::user("Hi")]);
        assert!(prompt.contains("greeter"));
        assert!(prompt.contains("Greet by name."));
        assert!(prompt.contains("User: Hi"));
    }

    #[test]
    fn extracts_json_from_fenced_or_prose_text() {
        assert_eq!(
            extract_json_object("```json\n{\"value\": true}\n```"),
            Some("{\"value\": true}")
        );
        assert_eq!(
            extract_json_object("Sure! {\"value\": 8, \"reason\": \"x\"} done"),
            Some("{\"value\": 8, \"reason\": \"x\"}")
        );
        assert_eq!(extract_json_object("no json here"), None);
    }

    #[test]
    fn parses_boolean_and_numeric_verdicts() {
        let b = parse_verdict(JudgeKind::Boolean, "{\"value\": true, \"reason\": \"ok\"}").unwrap();
        assert!(matches!(b.value, JudgeValue::Bool(true)));
        assert_eq!(b.reason, "ok");

        let n =
            parse_verdict(JudgeKind::Numeric, "{\"value\": 8.5, \"reason\": \"good\"}").unwrap();
        assert!(matches!(n.value, JudgeValue::Number(v) if (v - 8.5).abs() < f64::EPSILON));
    }

    #[test]
    fn verdict_with_wrong_value_type_errors() {
        assert!(parse_verdict(JudgeKind::Boolean, "{\"value\": 3}").is_err());
        assert!(parse_verdict(JudgeKind::Numeric, "{\"value\": true}").is_err());
        assert!(parse_verdict(JudgeKind::Boolean, "no json").is_err());
    }
}
