//! Configuration: the provider command, the default platforms and models a run
//! fans out across, and the model used for natural-language evals.
//!
//! Config is loaded from a YAML file (default `skilltest.yaml`) and then refined
//! by CLI overrides (see [`Config::apply_overrides`]).

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// The full configuration for a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// The provider command as an argv vector. The default expects
    /// [`oneharness`](https://github.com/nickderobertis/oneharness) on `PATH`.
    /// Tests point this at a deterministic fake provider.
    pub provider: Vec<String>,
    /// Harness platforms a case runs on (e.g. `claude-code`, `cursor`).
    pub platforms: Vec<String>,
    /// Models a case runs on (e.g. `claude-opus-4-8`).
    pub models: Vec<String>,
    /// Model used for natural-language evals and the simulated user. Falls back
    /// to the first entry of `models` when empty.
    pub judge_model: String,
    /// Default cap on assistant turns for multi-turn cases. A case may lower it.
    pub max_turns: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            provider: vec!["oneharness".to_string()],
            platforms: vec!["claude-code".to_string()],
            models: vec!["claude-opus-4-8".to_string()],
            judge_model: String::new(),
            max_turns: 8,
        }
    }
}

/// CLI-supplied overrides. `None`/empty fields leave the config value in place.
#[derive(Debug, Clone, Default)]
pub struct Overrides {
    pub provider: Option<Vec<String>>,
    pub platforms: Vec<String>,
    pub models: Vec<String>,
    pub judge_model: Option<String>,
    pub max_turns: Option<u32>,
}

impl Config {
    /// Load configuration from `path`. The standard config filename is
    /// `skilltest.yaml`.
    ///
    /// # Errors
    /// [`Error::Io`] if the file cannot be read, [`Error::Yaml`] if it does not
    /// parse, and [`Error::Invalid`] if it parses but is internally
    /// inconsistent (see [`Config::validate`]).
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let config: Config = serde_yaml::from_str(&text).map_err(|source| Error::Yaml {
            path: path.to_path_buf(),
            source,
        })?;
        config.validate()?;
        Ok(config)
    }

    /// Load `path` if it exists, otherwise return [`Config::default`]. This lets
    /// the CLI run against an explicit config or fall back to sane defaults plus
    /// CLI flags.
    ///
    /// # Errors
    /// Same as [`Config::load`] when the file is present but invalid.
    pub fn load_or_default(path: &Path) -> Result<Self> {
        if path.is_file() {
            Self::load(path)
        } else {
            Ok(Self::default())
        }
    }

    /// Apply CLI overrides in place, then re-validate.
    ///
    /// # Errors
    /// [`Error::Invalid`] if the merged configuration is inconsistent.
    pub fn apply_overrides(&mut self, overrides: Overrides) -> Result<()> {
        if let Some(provider) = overrides.provider {
            self.provider = provider;
        }
        if !overrides.platforms.is_empty() {
            self.platforms = overrides.platforms;
        }
        if !overrides.models.is_empty() {
            self.models = overrides.models;
        }
        if let Some(judge) = overrides.judge_model {
            self.judge_model = judge;
        }
        if let Some(max_turns) = overrides.max_turns {
            self.max_turns = max_turns;
        }
        self.validate()
    }

    /// The model used for evals and the simulated user: `judge_model` if set,
    /// otherwise the first configured model.
    #[must_use]
    pub fn effective_judge_model(&self) -> &str {
        if self.judge_model.is_empty() {
            self.models.first().map_or("", String::as_str)
        } else {
            &self.judge_model
        }
    }

    /// Check internal consistency.
    ///
    /// # Errors
    /// [`Error::Invalid`] when the provider is empty or no platform/model is set.
    pub fn validate(&self) -> Result<()> {
        if self.provider.is_empty() {
            return Err(Error::Invalid(
                "config `provider` must name a command (e.g. [\"oneharness\"])".into(),
            ));
        }
        if self.platforms.is_empty() {
            return Err(Error::Invalid(
                "config `platforms` must list at least one harness platform".into(),
            ));
        }
        if self.models.is_empty() {
            return Err(Error::Invalid(
                "config `models` must list at least one model".into(),
            ));
        }
        if self.max_turns == 0 {
            return Err(Error::Invalid(
                "config `max_turns` must be at least 1".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        Config::default().validate().unwrap();
    }

    #[test]
    fn overrides_replace_lists() {
        let mut config = Config::default();
        config
            .apply_overrides(Overrides {
                models: vec!["a".into(), "b".into()],
                ..Default::default()
            })
            .unwrap();
        assert_eq!(config.models, vec!["a", "b"]);
        // Platforms untouched.
        assert_eq!(config.platforms, vec!["claude-code"]);
    }

    #[test]
    fn judge_model_falls_back_to_first_model() {
        let config = Config::default();
        assert_eq!(config.effective_judge_model(), "claude-opus-4-8");
    }

    #[test]
    fn empty_models_is_invalid() {
        let mut config = Config::default();
        config.models.clear();
        assert!(config.validate().is_err());
    }
}
