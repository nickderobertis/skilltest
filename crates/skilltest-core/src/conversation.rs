//! The conversation model: the transcript that flows between the runner and the
//! provider, and is ultimately handed to evals.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Who produced a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// The (real or simulated) user driving the skill.
    User,
    /// The skill / assistant under test.
    Assistant,
    /// System-level framing, if a provider chooses to surface it.
    System,
}

/// One normalized tool-call / action event the skill took during a turn, lifted
/// from oneharness's `events` array (its `--events` output). Harness-agnostic, so
/// a behavioral eval can assert on *what the skill did* — shell commands, file
/// edits, tool uses — across any harness, not just the final text. Mirrors the
/// oneharness `ActionEvent` shape; `input` is the structured, tool-shaped args so
/// a consumer can match on the command string or file path without re-parsing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToolEvent {
    /// `tool_call` (the skill invoked a tool) or `tool_result` (the observation).
    pub kind: String,
    /// Normalized tool name where knowable (e.g. `bash`, `edit_file`); `null` for
    /// a `tool_result` or when the harness did not name it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Structured tool arguments (the command, the file path); `null` when none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Value>,
    /// The result/observation text, when the transcript exposed it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// Position within the turn, so ordering ("did X before Y") is expressible.
    #[serde(default)]
    pub index: usize,
}

/// A single turn in the conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Message {
    pub role: Role,
    pub content: String,
    /// The normalized tool events the skill took producing this turn (assistant
    /// turns only, and only when the harness exposed a tool transcript via
    /// oneharness `--events`). Empty otherwise. Feeds behavioral evals.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<ToolEvent>,
}

impl Message {
    /// Build a user message.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
            events: Vec::new(),
        }
    }

    /// Build an assistant message.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            events: Vec::new(),
        }
    }

    /// Attach the turn's normalized tool events (builder style).
    #[must_use]
    pub fn with_events(mut self, events: Vec<ToolEvent>) -> Self {
        self.events = events;
        self
    }
}

/// An ordered list of messages. Thin wrapper so the type reads clearly at call
/// sites and so we can grow conversation-level helpers without churn.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Transcript {
    pub messages: Vec<Message>,
}

impl Transcript {
    /// Start a transcript from the initial user input given to the skill.
    pub fn from_input(input: impl Into<String>) -> Self {
        Self {
            messages: vec![Message::user(input)],
        }
    }

    /// Append a message.
    pub fn push(&mut self, message: Message) {
        self.messages.push(message);
    }

    /// Number of assistant turns produced so far.
    #[must_use]
    pub fn assistant_turns(&self) -> usize {
        self.messages
            .iter()
            .filter(|m| m.role == Role::Assistant)
            .count()
    }
}
