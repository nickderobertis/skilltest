//! Test cases: the YAML a user writes to describe one test of a skill — the
//! initial data to hand the skill, an optional simulated user for multi-turn
//! runs, and the evals that decide pass/fail.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::eval::Eval;

/// The simulated-user block that turns a single-turn case into a multi-turn one.
/// When present, after each assistant turn the runner asks the provider to play
/// the user (guided by `persona`) until `done_when` holds or `max_turns` is hit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimulatedUser {
    /// Instructions describing how the simulated user should behave.
    pub persona: String,
    /// A plain-English condition; when the judge decides it holds, the
    /// conversation ends. Optional — without it the run ends at `max_turns` or
    /// when the skill reports itself done.
    #[serde(default)]
    pub done_when: Option<String>,
    /// Per-case override of the global assistant-turn cap.
    #[serde(default)]
    pub max_turns: Option<u32>,
}

/// One test case.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestCase {
    /// Human-readable name (defaults to the file stem when loaded from a file).
    #[serde(default)]
    pub name: String,
    /// Path to the skill directory under test, relative to the test-case file.
    pub skill: PathBuf,
    /// The initial data/prompt handed to the skill as the first user message.
    pub input: String,
    /// Present for multi-turn cases; absent for single-turn.
    #[serde(default)]
    pub user: Option<SimulatedUser>,
    /// The evals that decide whether this case passes. Must be non-empty.
    pub evals: Vec<Eval>,
}

impl TestCase {
    /// Load a test case from a YAML file. The `name` defaults to the file stem
    /// and `skill` is resolved relative to the file's directory.
    ///
    /// # Errors
    /// [`Error::Io`] if the file cannot be read, [`Error::Yaml`] on parse
    /// failure, and [`Error::Invalid`] if the case is internally inconsistent.
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let mut case: TestCase = serde_yaml::from_str(&text).map_err(|source| Error::Yaml {
            path: path.to_path_buf(),
            source,
        })?;
        if case.name.is_empty() {
            case.name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("case")
                .to_string();
        }
        if let Some(parent) = path.parent() {
            if case.skill.is_relative() {
                case.skill = parent.join(&case.skill);
            }
        }
        case.validate()?;
        Ok(case)
    }

    /// Whether this is a multi-turn case (has a simulated user).
    #[must_use]
    pub fn is_multi_turn(&self) -> bool {
        self.user.is_some()
    }

    /// Validate the case's structure and every eval.
    ///
    /// # Errors
    /// [`Error::Invalid`] when input/evals are empty or an eval is malformed.
    pub fn validate(&self) -> Result<()> {
        if self.input.trim().is_empty() {
            return Err(Error::Invalid(format!(
                "test case `{}` has an empty `input`",
                self.name
            )));
        }
        if self.evals.is_empty() {
            return Err(Error::Invalid(format!(
                "test case `{}` defines no `evals`",
                self.name
            )));
        }
        for eval in &self.evals {
            eval.validate()?;
        }
        if let Some(user) = &self.user {
            if user.persona.trim().is_empty() {
                return Err(Error::Invalid(format!(
                    "test case `{}` has a `user` block with an empty `persona`",
                    self.name
                )));
            }
            if user.max_turns == Some(0) {
                return Err(Error::Invalid(format!(
                    "test case `{}` sets `user.max_turns` to 0",
                    self.name
                )));
            }
        }
        Ok(())
    }
}

/// Discover test-case files: either a single `.yaml`/`.yml` file or every such
/// file directly inside a directory (sorted for deterministic ordering).
///
/// # Errors
/// [`Error::Io`] if a directory cannot be read, [`Error::Invalid`] if the path
/// matches nothing usable.
pub fn discover_cases(path: &Path) -> Result<Vec<PathBuf>> {
    if path.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }
    if path.is_dir() {
        let entries = std::fs::read_dir(path).map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let mut files: Vec<PathBuf> = entries
            .filter_map(std::result::Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && matches!(p.extension().and_then(|s| s.to_str()), Some("yaml" | "yml"))
            })
            .collect();
        files.sort();
        if files.is_empty() {
            return Err(Error::Invalid(format!(
                "no .yaml test cases found in {}",
                path.display()
            )));
        }
        return Ok(files);
    }
    Err(Error::Invalid(format!(
        "path does not exist: {}",
        path.display()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::Eval;

    #[test]
    fn parses_single_turn_case() {
        let yaml = r#"
skill: ./greeter
input: "Greet Dr. Smith"
evals:
  - type: boolean
    criterion: "greets Dr. Smith by name"
"#;
        let case: TestCase = serde_yaml::from_str(yaml).unwrap();
        assert!(!case.is_multi_turn());
        assert_eq!(case.evals.len(), 1);
        assert!(matches!(case.evals[0], Eval::Boolean { .. }));
    }

    #[test]
    fn parses_multi_turn_case() {
        let yaml = r#"
name: booking
skill: ./booker
input: "I want to book an appointment"
user:
  persona: "You are a terse patient."
  done_when: "the assistant has confirmed a booking"
  max_turns: 5
evals:
  - type: numeric
    criterion: "how clearly was the appointment confirmed"
    min: 0
    max: 10
    threshold: 7
"#;
        let case: TestCase = serde_yaml::from_str(yaml).unwrap();
        assert!(case.is_multi_turn());
        assert_eq!(case.user.as_ref().unwrap().max_turns, Some(5));
        case.validate().unwrap();
    }

    #[test]
    fn empty_evals_is_invalid() {
        let yaml = "skill: ./x\ninput: hi\nevals: []\n";
        let case: TestCase = serde_yaml::from_str(yaml).unwrap();
        assert!(case.validate().is_err());
    }

    #[test]
    fn unknown_field_is_rejected() {
        let yaml = "skill: ./x\ninput: hi\nbogus: 1\nevals: []\n";
        assert!(serde_yaml::from_str::<TestCase>(yaml).is_err());
    }
}
