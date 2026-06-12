//! The conversation model: the transcript that flows between the runner and the
//! provider, and is ultimately handed to evals.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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

/// A single turn in the conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

impl Message {
    /// Build a user message.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }

    /// Build an assistant message.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }
}

/// An ordered list of messages. Thin wrapper so the type reads clearly at call
/// sites and so we can grow conversation-level helpers without churn.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
