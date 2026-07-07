//! Error type for the core library. The mapping from these errors to process
//! exit codes lives in the CLI (see `exit.rs` for the documented codes).

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Result alias used throughout the core library.
pub type Result<T> = std::result::Result<T, Error>;

/// Structured classification of a provider failure.
///
/// This is the machine-readable category SDK/plugin consumers branch on instead
/// of matching substrings in the human message (the reason this type exists: a
/// timeout is [`ProviderErrorKind::Timeout`], not `message.contains("timed
/// out")`). It is part of the JSON error contract — it serializes as a
/// snake_case string and is re-exported through every SDK's generated models, so
/// the vocabulary here is the vocabulary consumers see.
///
/// The set is closed on purpose so the generated SDK types are exhaustive
/// unions; a failure skilltest cannot map to a specific category (e.g. a
/// provider or oneharness failure label added upstream that this version does
/// not recognize) becomes [`ProviderErrorKind::Other`] rather than a new,
/// unhandled string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProviderErrorKind {
    /// Authentication/authorization failed (missing or rejected credentials).
    Auth,
    /// The provider rate-limited the call; a backoff-and-retry may succeed.
    RateLimit,
    /// The harness/vendor does not recognize the requested model.
    ModelNotFound,
    /// The account's quota or billing limit is exhausted.
    Quota,
    /// A transient server-side overload; retried internally where possible.
    Overloaded,
    /// The call exceeded its deadline (harness `--timeout`, curl `--max-time`).
    Timeout,
    /// The provider process could not be started (binary missing, not runnable).
    Spawn,
    /// The provider ran but produced output that violated the protocol
    /// (unparseable envelope, missing results, malformed verdict).
    Protocol,
    /// A classified failure that does not map to a more specific category. The
    /// human `message` carries the specifics.
    Other,
}

impl ProviderErrorKind {
    /// Map a raw failure label — an oneharness `failure_kind`, or a vendor error
    /// class skilltest already normalized to one of these snake_case strings — to
    /// a category. Unrecognized labels become [`ProviderErrorKind::Other`].
    #[must_use]
    pub fn classify(raw: &str) -> Self {
        match raw {
            "auth" => Self::Auth,
            "rate_limit" => Self::RateLimit,
            "model_not_found" => Self::ModelNotFound,
            "quota" => Self::Quota,
            "overloaded" => Self::Overloaded,
            "timeout" => Self::Timeout,
            "spawn" => Self::Spawn,
            "protocol" => Self::Protocol,
            _ => Self::Other,
        }
    }

    /// The stable snake_case wire string for this kind (matches serde).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auth => "auth",
            Self::RateLimit => "rate_limit",
            Self::ModelNotFound => "model_not_found",
            Self::Quota => "quota",
            Self::Overloaded => "overloaded",
            Self::Timeout => "timeout",
            Self::Spawn => "spawn",
            Self::Protocol => "protocol",
            Self::Other => "other",
        }
    }
}

impl std::fmt::Display for ProviderErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

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
    /// set, classifies the failure (see [`ProviderErrorKind`]) so consumers can
    /// distinguish a broken environment from a broken skill without parsing
    /// `message`.
    #[error("provider error ({context}): {message}")]
    Provider {
        context: String,
        message: String,
        kind: Option<ProviderErrorKind>,
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

    /// Construct a classified [`Error::Provider`] (e.g.
    /// `kind = ProviderErrorKind::Auth`).
    pub fn provider_classified(
        context: impl Into<String>,
        message: impl std::fmt::Display,
        kind: ProviderErrorKind,
    ) -> Self {
        Error::Provider {
            context: context.into(),
            message: message.to_string(),
            kind: Some(kind),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every classified kind's `as_str`, its serde string, its `Display`, and
    /// `classify` (the inverse) must agree — this is the vocabulary the JSON
    /// error contract exposes to SDK consumers, so the three views can't drift.
    #[test]
    fn provider_error_kind_string_forms_agree() {
        let all = [
            ProviderErrorKind::Auth,
            ProviderErrorKind::RateLimit,
            ProviderErrorKind::ModelNotFound,
            ProviderErrorKind::Quota,
            ProviderErrorKind::Overloaded,
            ProviderErrorKind::Timeout,
            ProviderErrorKind::Spawn,
            ProviderErrorKind::Protocol,
            ProviderErrorKind::Other,
        ];
        for kind in all {
            let wire = kind.as_str();
            // serde serializes to the same bare string.
            assert_eq!(serde_json::to_value(kind).unwrap(), serde_json::json!(wire));
            // Display matches as_str.
            assert_eq!(kind.to_string(), wire);
            // classify is the inverse for known labels.
            assert_eq!(ProviderErrorKind::classify(wire), kind);
        }
    }

    #[test]
    fn classify_maps_unknown_labels_to_other() {
        assert_eq!(
            ProviderErrorKind::classify("brand_new_upstream_kind"),
            ProviderErrorKind::Other
        );
    }
}
