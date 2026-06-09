//! The provider boundary. `skilltest` never talks to a model directly; it shells
//! out to a provider command (default `oneharness`) that speaks a small
//! JSON-lines protocol — one request object in on stdin, one response object out
//! on stdout, per invocation. See `docs/protocol.md` for the wire format.
//!
//! The [`Provider`] trait abstracts that boundary so the runner can be unit
//! tested against an in-memory fake, while [`CommandProvider`] is the real
//! subprocess implementation used in production and by the e2e suite (pointed at
//! the deterministic `skilltest-fake-provider`).

use std::io::Write as _;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::conversation::Message;
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
}
