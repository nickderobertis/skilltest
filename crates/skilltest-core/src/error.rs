//! Error type for the core library. The mapping from these errors to process
//! exit codes lives in the CLI (see `exit.rs` for the documented codes).

use std::path::PathBuf;

/// Result alias used throughout the core library.
pub type Result<T> = std::result::Result<T, Error>;

/// Everything that can go wrong while loading configuration, parsing skill or
/// test-case definitions, talking to a provider, or running evals.
///
/// Variants are grouped so the CLI can map them onto stable exit codes: input
/// problems (`Config`, `Yaml`, `Skill`, `Validation`) are the user's to fix;
/// `Provider` problems are environmental.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A file the user pointed us at could not be read.
    #[error("could not read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// A YAML document (config or test case) failed to parse.
    #[error("invalid YAML in {path}: {source}")]
    Yaml {
        path: PathBuf,
        #[source]
        source: serde_yaml::Error,
    },

    /// A test case or config was syntactically valid YAML but semantically
    /// wrong (e.g. a numeric eval with `min > max`).
    #[error("invalid test definition: {0}")]
    Invalid(String),

    /// The provider command could not be spawned or did not behave. `kind`, when
    /// set, classifies the failure (e.g. `"auth"`, `"rate_limit"`,
    /// `"model_not_found"`, `"quota"`) so the CLI can distinguish a broken
    /// environment from a broken skill.
    #[error("provider error ({context}): {message}")]
    Provider {
        context: String,
        message: String,
        kind: Option<String>,
    },

    /// A skill definition failed validation. Carries the human-readable
    /// findings so the CLI can print them.
    #[error("skill validation failed with {} finding(s)", .0.len())]
    Validation(Vec<String>),
}

impl Error {
    /// Construct a [`Error::Provider`] with no classification.
    pub fn provider(context: impl Into<String>, message: impl std::fmt::Display) -> Self {
        Error::Provider {
            context: context.into(),
            message: message.to_string(),
            kind: None,
        }
    }

    /// Construct a classified [`Error::Provider`] (e.g. `kind = "auth"`).
    pub fn provider_classified(
        context: impl Into<String>,
        message: impl std::fmt::Display,
        kind: impl Into<String>,
    ) -> Self {
        Error::Provider {
            context: context.into(),
            message: message.to_string(),
            kind: Some(kind.into()),
        }
    }
}
