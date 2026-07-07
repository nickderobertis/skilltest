//! Configuration: which provider runs skills, the default platforms and models a
//! run fans out across, and the model used for natural-language evals.
//!
//! Config is loaded from a YAML file (default `skilltest.yaml`) and then refined
//! by CLI overrides (see [`Config::apply_overrides`]).

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::mock::MockDecl;

/// Environment variable the SDKs set to the `oneharness` binary they bundle (the
/// `oneharness-cli` PyPI/npm package installed alongside the SDK), so a run
/// resolves oneharness with no `PATH` setup. It is only a *default*: an explicit
/// config `provider.bin` (or the `--oneharness-bin` flag) still wins.
pub const ONEHARNESS_BIN_ENV: &str = "SKILLTEST_ONEHARNESS_BIN";

fn default_oneharness_bin() -> String {
    match std::env::var(ONEHARNESS_BIN_ENV) {
        Ok(bin) if !bin.trim().is_empty() => bin,
        _ => "oneharness".to_string(),
    }
}

fn default_judge_harness() -> String {
    "claude-code".to_string()
}

fn default_timeout_secs() -> u64 {
    120
}

fn default_api_timeout_secs() -> u64 {
    60
}

fn default_curl_bin() -> String {
    "curl".to_string()
}

fn default_true() -> bool {
    true
}

/// Settings for the default [`oneharness`](https://github.com/nickderobertis/oneharness)
/// provider, which runs each prompt on a harness via `oneharness run`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OneharnessConfig {
    /// The `oneharness` binary. When unset, defaults to `$SKILLTEST_ONEHARNESS_BIN`
    /// (the path the SDKs point at their bundled `oneharness-cli`), else
    /// `oneharness` resolved on `PATH`.
    #[serde(default = "default_oneharness_bin")]
    pub bin: String,
    /// The harness used for evals and the simulated user (kept independent of the
    /// harness under test, so the evaluator does not vary with the matrix).
    #[serde(default = "default_judge_harness")]
    pub judge_harness: String,
    /// Per-call timeout passed through to `oneharness run --timeout`.
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
}

impl Default for OneharnessConfig {
    fn default() -> Self {
        Self {
            bin: default_oneharness_bin(),
            judge_harness: default_judge_harness(),
            timeout_secs: default_timeout_secs(),
        }
    }
}

/// Settings for a custom provider command speaking the JSON-lines protocol (see
/// `docs/protocol.md`). Used by the bundled `skilltest-fake-provider` and any
/// provider you write yourself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandConfig {
    /// The provider command as an argv vector.
    pub command: Vec<String>,
}

/// Which provider backs a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ProviderConfig {
    /// Run skills through `oneharness` (the default).
    Oneharness(OneharnessConfig),
    /// Run a custom command speaking the JSON-lines protocol.
    Command(CommandConfig),
}

impl Default for ProviderConfig {
    fn default() -> Self {
        ProviderConfig::Oneharness(OneharnessConfig::default())
    }
}

/// Which model vendor's API the direct-API judge talks to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApiVendor {
    /// Anthropic Messages API (`POST /v1/messages`).
    Anthropic,
    /// OpenAI Chat Completions API (`POST /v1/chat/completions`).
    Openai,
}

/// Settings for judging evals and the simulated user with a direct model API
/// call instead of running them through a harness. This trades the harness's
/// auth-portability for a single fast HTTP round trip per judge call (no
/// agent-loop cold start), with normalized token usage surfaced into the report.
///
/// The judge *model* is the run's `judge_model` (it must be a valid API model
/// id for the chosen `vendor`, e.g. `claude-opus-4-8` or `gpt-4o`); only the
/// transport is configured here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApiJudgeConfig {
    /// Which vendor's API to call.
    pub vendor: ApiVendor,
    /// Environment variable holding the API key. Defaults to `ANTHROPIC_API_KEY`
    /// or `OPENAI_API_KEY` by vendor. The key is read at run time and never
    /// stored in config.
    #[serde(default)]
    pub api_key_env: Option<String>,
    /// Override the API endpoint (e.g. a proxy or an OpenAI-compatible gateway).
    /// Defaults to the vendor's standard endpoint.
    #[serde(default)]
    pub base_url: Option<String>,
    /// Per-call timeout in seconds, passed to `curl --max-time`.
    #[serde(default = "default_api_timeout_secs")]
    pub timeout_secs: u64,
    /// The `curl` binary (resolved on `PATH`).
    #[serde(default = "default_curl_bin")]
    pub curl_bin: String,
    /// Constrain the judge's verdict to the `{value, reason}` JSON schema via the
    /// vendor's structured-outputs feature (Anthropic `output_config.format`,
    /// OpenAI `response_format: json_schema`). On by default — it removes a class
    /// of judge-parse fragility. Turn it off for a model/endpoint that doesn't
    /// support structured outputs (the tolerant `{…}` extraction still applies).
    #[serde(default = "default_true")]
    pub strict_json: bool,
}

/// How evals and the simulated user are judged, independent of the provider that
/// runs the skill. Absent (the default) means the run's provider judges too
/// (e.g. the oneharness `judge_harness`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum JudgeConfig {
    /// Judge with a direct model API call (see [`ApiJudgeConfig`]).
    Api(ApiJudgeConfig),
}

/// The full configuration for a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// The provider that executes skills and evals.
    pub provider: ProviderConfig,
    /// Harness platforms a case runs on (e.g. `claude-code`, `codex`).
    pub platforms: Vec<String>,
    /// Models a case runs on (must be valid for the chosen harness, e.g.
    /// `sonnet`/`haiku` for `claude-code`).
    pub models: Vec<String>,
    /// Model used for natural-language evals and the simulated user. Falls back
    /// to the first entry of `models` when empty.
    pub judge_model: String,
    /// Default cap on assistant turns for multi-turn cases. A case may lower it.
    pub max_turns: u32,
    /// Optional judge backend that overrides how evals and the simulated user are
    /// scored, independent of the skill-running provider. When `None`, the
    /// provider judges (e.g. the oneharness `judge_harness`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub judge: Option<JudgeConfig>,
    /// Shared mock/spy declarations, prepended to every case's own `mocks`
    /// (first match wins, so these shadow the case's). Populated by the CLI's
    /// `--mocks <file>` — how the SDKs deliver code-level mocks — or written
    /// here directly for suite-wide rules.
    #[serde(default)]
    pub mocks: Vec<MockDecl>,
    /// Force the mock/spy observation channel on for every run, even for cases
    /// that declare no `mocks` (the CLI's `--spy`; how SDK spies get records).
    #[serde(default)]
    pub spy: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            provider: ProviderConfig::default(),
            platforms: vec!["claude-code".to_string()],
            models: vec!["claude-opus-4-8".to_string()],
            judge_model: String::new(),
            max_turns: 8,
            judge: None,
            mocks: Vec::new(),
            spy: false,
        }
    }
}

/// CLI-supplied overrides. `None`/empty fields leave the config value in place.
#[derive(Debug, Clone, Default)]
pub struct Overrides {
    /// If set, switch to a [`ProviderConfig::Command`] with this argv.
    pub command_provider: Option<Vec<String>>,
    /// Override the `oneharness` binary (only applies to the oneharness provider).
    pub oneharness_bin: Option<String>,
    /// Override the judge harness (only applies to the oneharness provider).
    pub judge_harness: Option<String>,
    /// Override the per-call timeout (only applies to the oneharness provider).
    pub timeout_secs: Option<u64>,
    pub platforms: Vec<String>,
    pub models: Vec<String>,
    pub judge_model: Option<String>,
    pub max_turns: Option<u32>,
    /// Mock/spy declarations prepended before any config-level ones (the CLI's
    /// `--mocks` file / SDK-passed mocks).
    pub mocks: Vec<MockDecl>,
    /// Force the observation channel on (the CLI's `--spy`).
    pub spy: bool,
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

    /// Load `path` if it exists, otherwise return [`Config::default`].
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
        if let Some(command) = overrides.command_provider {
            self.provider = ProviderConfig::Command(CommandConfig { command });
        } else if let ProviderConfig::Oneharness(oh) = &mut self.provider {
            if let Some(bin) = overrides.oneharness_bin {
                oh.bin = bin;
            }
            if let Some(judge_harness) = overrides.judge_harness {
                oh.judge_harness = judge_harness;
            }
            if let Some(timeout) = overrides.timeout_secs {
                oh.timeout_secs = timeout;
            }
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
        if !overrides.mocks.is_empty() {
            // Override mocks come first: first match wins, so the most local
            // declaration (CLI/SDK) shadows a config-level one.
            let mut mocks = overrides.mocks;
            mocks.append(&mut self.mocks);
            self.mocks = mocks;
        }
        self.spy = self.spy || overrides.spy;
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
    /// [`Error::Invalid`] when the provider is misconfigured or no
    /// platform/model is set.
    pub fn validate(&self) -> Result<()> {
        match &self.provider {
            ProviderConfig::Oneharness(oh) => {
                if oh.bin.trim().is_empty() {
                    return Err(Error::Invalid(
                        "config `provider.bin` must name the oneharness binary".into(),
                    ));
                }
                if oh.judge_harness.trim().is_empty() {
                    return Err(Error::Invalid(
                        "config `provider.judge_harness` must name a harness".into(),
                    ));
                }
                if oh.timeout_secs == 0 {
                    return Err(Error::Invalid(
                        "config `provider.timeout_secs` must be at least 1".into(),
                    ));
                }
            }
            ProviderConfig::Command(c) => {
                if c.command.is_empty() {
                    return Err(Error::Invalid(
                        "config `provider.command` must name a command".into(),
                    ));
                }
            }
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
        if let Some(JudgeConfig::Api(api)) = &self.judge {
            if api.timeout_secs == 0 {
                return Err(Error::Invalid(
                    "config `judge.timeout_secs` must be at least 1".into(),
                ));
            }
            if api.curl_bin.trim().is_empty() {
                return Err(Error::Invalid(
                    "config `judge.curl_bin` must name the curl binary".into(),
                ));
            }
        }
        for (i, decl) in self.mocks.iter().enumerate() {
            let label = decl.name.clone().unwrap_or_else(|| format!("#{i}"));
            decl.validate(&format!("config mock `{label}`"))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid_and_use_oneharness() {
        let config = Config::default();
        config.validate().unwrap();
        assert!(matches!(config.provider, ProviderConfig::Oneharness(_)));
    }

    #[test]
    fn command_override_switches_provider() {
        let mut config = Config::default();
        config
            .apply_overrides(Overrides {
                command_provider: Some(vec!["fake".into()]),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            config.provider,
            ProviderConfig::Command(CommandConfig {
                command: vec!["fake".into()]
            })
        );
    }

    #[test]
    fn oneharness_bin_override_applies() {
        let mut config = Config::default();
        config
            .apply_overrides(Overrides {
                oneharness_bin: Some("/tmp/oneharness".into()),
                ..Default::default()
            })
            .unwrap();
        let ProviderConfig::Oneharness(oh) = &config.provider else {
            panic!("expected oneharness provider");
        };
        assert_eq!(oh.bin, "/tmp/oneharness");
    }

    #[test]
    fn oneharness_bin_defaults_to_env_when_set() {
        // The SDKs export SKILLTEST_ONEHARNESS_BIN to their bundled oneharness;
        // a config that omits `bin` picks it up. (nextest isolates each test in
        // its own process, so this env write does not leak to other tests.)
        std::env::set_var(ONEHARNESS_BIN_ENV, "/opt/bundled/oneharness");
        let yaml = "provider:\n  kind: oneharness\n  judge_harness: codex\n";
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        let ProviderConfig::Oneharness(oh) = &config.provider else {
            panic!("expected oneharness provider");
        };
        assert_eq!(oh.bin, "/opt/bundled/oneharness");
        std::env::remove_var(ONEHARNESS_BIN_ENV);
    }

    #[test]
    fn explicit_bin_wins_over_env_default() {
        // An explicit `provider.bin` is a present field, so serde never calls the
        // env-aware default — the user's choice is honored over the bundled one.
        std::env::set_var(ONEHARNESS_BIN_ENV, "/opt/bundled/oneharness");
        let yaml = "provider:\n  kind: oneharness\n  bin: /custom/oneharness\n";
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        let ProviderConfig::Oneharness(oh) = &config.provider else {
            panic!("expected oneharness provider");
        };
        assert_eq!(oh.bin, "/custom/oneharness");
        std::env::remove_var(ONEHARNESS_BIN_ENV);
    }

    #[test]
    fn oneharness_bin_defaults_to_path_name_without_env() {
        std::env::remove_var(ONEHARNESS_BIN_ENV);
        let yaml = "provider:\n  kind: oneharness\n";
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        let ProviderConfig::Oneharness(oh) = &config.provider else {
            panic!("expected oneharness provider");
        };
        assert_eq!(oh.bin, "oneharness");
    }

    #[test]
    fn parses_command_provider_yaml() {
        let yaml = "provider:\n  kind: command\n  command: [\"prov\", \"--flag\"]\n";
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            config.provider,
            ProviderConfig::Command(CommandConfig {
                command: vec!["prov".into(), "--flag".into()]
            })
        );
    }

    #[test]
    fn parses_oneharness_provider_yaml() {
        let yaml = "provider:\n  kind: oneharness\n  bin: oh\n  judge_harness: codex\n";
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        let ProviderConfig::Oneharness(oh) = &config.provider else {
            panic!("expected oneharness provider");
        };
        assert_eq!(oh.bin, "oh");
        assert_eq!(oh.judge_harness, "codex");
        // Unspecified fields fall back to defaults.
        assert_eq!(oh.timeout_secs, 120);
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

    #[test]
    fn parses_api_judge_config() {
        let yaml = "\
provider:\n  kind: oneharness\njudge:\n  kind: api\n  vendor: anthropic\n  timeout_secs: 30\n";
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        let Some(JudgeConfig::Api(api)) = &config.judge else {
            panic!("expected an api judge");
        };
        assert_eq!(api.vendor, ApiVendor::Anthropic);
        assert_eq!(api.timeout_secs, 30);
        // Unspecified fields fall back to defaults.
        assert_eq!(api.curl_bin, "curl");
        assert!(api.api_key_env.is_none());
        assert!(api.strict_json, "strict JSON is on by default");
        config.validate().unwrap();
    }

    #[test]
    fn api_judge_zero_timeout_is_invalid() {
        let yaml = "judge:\n  kind: api\n  vendor: openai\n  timeout_secs: 0\n";
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn default_config_has_no_judge_override() {
        assert!(Config::default().judge.is_none());
    }

    /// Write `yaml` to a unique temp file and return its path.
    fn config_file(tag: &str, yaml: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "skilltest-config-{}-{tag}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("skilltest.yaml");
        std::fs::write(&path, yaml).unwrap();
        path
    }

    #[test]
    fn load_reads_and_validates_a_file() {
        let path = config_file(
            "load",
            "provider:\n  kind: command\n  command: [\"prov\"]\nplatforms: [demo]\nmodels: [m]\n",
        );
        let config = Config::load(&path).unwrap();
        assert_eq!(config.platforms, vec!["demo".to_string()]);
        assert!(matches!(config.provider, ProviderConfig::Command(_)));
    }

    #[test]
    fn load_missing_file_is_io_error() {
        let path = std::env::temp_dir().join(format!("skilltest-none-{}.yaml", std::process::id()));
        assert!(matches!(Config::load(&path), Err(Error::Io { .. })));
    }

    #[test]
    fn load_malformed_yaml_is_yaml_error() {
        let path = config_file("bad", "platforms: [unterminated\n");
        assert!(matches!(Config::load(&path), Err(Error::Yaml { .. })));
    }

    #[test]
    fn load_inconsistent_config_is_invalid_error() {
        // Parses fine, but an empty command provider fails validation.
        let path = config_file(
            "inconsistent",
            "provider:\n  kind: command\n  command: []\n",
        );
        assert!(matches!(Config::load(&path), Err(Error::Invalid(_))));
    }

    #[test]
    fn load_or_default_returns_default_when_absent() {
        let path =
            std::env::temp_dir().join(format!("skilltest-absent-{}.yaml", std::process::id()));
        let config = Config::load_or_default(&path).unwrap();
        assert_eq!(config, Config::default());
    }

    #[test]
    fn load_or_default_loads_when_present() {
        let path = config_file("present", "platforms: [a, b]\nmodels: [m]\n");
        let config = Config::load_or_default(&path).unwrap();
        assert_eq!(config.platforms, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn overrides_apply_judge_harness_timeout_and_run_fields() {
        let mut config = Config::default();
        config
            .apply_overrides(Overrides {
                judge_harness: Some("codex".into()),
                timeout_secs: Some(45),
                platforms: vec!["p1".into(), "p2".into()],
                models: vec!["mod".into()],
                judge_model: Some("haiku".into()),
                max_turns: Some(3),
                ..Default::default()
            })
            .unwrap();
        let ProviderConfig::Oneharness(oh) = &config.provider else {
            panic!("still oneharness");
        };
        assert_eq!(oh.judge_harness, "codex");
        assert_eq!(oh.timeout_secs, 45);
        assert_eq!(config.platforms, vec!["p1".to_string(), "p2".to_string()]);
        assert_eq!(config.models, vec!["mod".to_string()]);
        assert_eq!(config.judge_model, "haiku");
        assert_eq!(config.max_turns, 3);
    }

    #[test]
    fn effective_judge_model_prefers_explicit_judge_model() {
        let config = Config {
            judge_model: "haiku".into(),
            ..Config::default()
        };
        assert_eq!(config.effective_judge_model(), "haiku");
    }

    #[test]
    fn validate_rejects_blank_oneharness_fields() {
        let mut config = Config::default();
        if let ProviderConfig::Oneharness(oh) = &mut config.provider {
            oh.bin = "  ".into();
        }
        assert!(config.validate().is_err());

        let mut config = Config::default();
        if let ProviderConfig::Oneharness(oh) = &mut config.provider {
            oh.judge_harness = "".into();
        }
        assert!(config.validate().is_err());

        let mut config = Config::default();
        if let ProviderConfig::Oneharness(oh) = &mut config.provider {
            oh.timeout_secs = 0;
        }
        assert!(config.validate().is_err());
    }

    #[test]
    fn validate_rejects_empty_platforms_and_zero_max_turns() {
        let mut config = Config::default();
        config.platforms.clear();
        assert!(config.validate().is_err());

        let config = Config {
            max_turns: 0,
            ..Config::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn validate_rejects_blank_api_judge_curl_bin() {
        let yaml = "judge:\n  kind: api\n  vendor: anthropic\n  curl_bin: \"  \"\n";
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn config_round_trips_through_yaml() {
        let config = Config {
            judge: Some(JudgeConfig::Api(ApiJudgeConfig {
                vendor: ApiVendor::Openai,
                api_key_env: Some("X".into()),
                base_url: None,
                timeout_secs: 30,
                curl_bin: "curl".into(),
                strict_json: false,
            })),
            ..Config::default()
        };
        let yaml = serde_yaml::to_string(&config).unwrap();
        let parsed: Config = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed, config);
    }
}
