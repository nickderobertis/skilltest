//! Test cases: the YAML a user writes to describe one test of a skill — the
//! initial data to hand the skill, an optional simulated user for multi-turn
//! runs, and the evals that decide pass/fail.

use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::eval::Eval;
use crate::mock::MockDecl;

/// The simulated-user block that turns a single-turn case into a multi-turn one.
/// When present, after each assistant turn the runner asks the provider to play
/// the user (guided by `persona`) until `done_when` holds or `max_turns` is hit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SimulatedUser {
    /// Instructions describing how the simulated user should behave.
    pub persona: String,
    /// A plain-English condition; when the judge decides it holds, the
    /// conversation ends. Optional — without it the run ends at `max_turns` or
    /// when the skill reports itself done.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done_when: Option<String>,
    /// Per-case override of the global assistant-turn cap.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<u32>,
}

/// One test case.
///
/// This type is the source of truth for the **input contract**
/// (`schemas/case.schema.json`): the shape a `--case-json` payload — and the
/// SDKs' generated case models — must have. Serialization skips
/// absent/default fields so the canonical JSON form is minimal; the SDK case
/// builders emit that same form (pinned by the kitchen-sink golden in
/// `tests/fixtures/contract/`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TestCase {
    /// Human-readable name (defaults to the file stem when loaded from a file).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Path to the skill directory under test, relative to the test-case file.
    pub skill: PathBuf,
    /// The initial data/prompt handed to the skill as the first user message.
    pub input: String,
    /// Present for multi-turn cases; absent for single-turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<SimulatedUser>,
    /// Mock/spy declarations for this case: a declaration with a `stub`/`deny`/
    /// `rewrite` action intercepts matching tool calls; one without observes
    /// only. `called`/`not_called` evals reference these by `name`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mocks: Vec<MockDecl>,
    /// Record every tool call through the mock/spy channel even with no
    /// `mocks` declared, so code-level consumers (the SDKs' spies) get records.
    /// Implied whenever `mocks` is non-empty.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub spy: bool,
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
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("case");
        let base = path.parent().unwrap_or_else(|| Path::new(""));
        case.finalize(base, stem)?;
        Ok(case)
    }

    /// Parse one or more fully-specified test cases from JSON — a single case
    /// object or an array — the shape the language SDKs emit when a case is
    /// built in code rather than written as YAML (`skilltest run --case-json`).
    ///
    /// Unlike [`load`](Self::load) these cases have no source file, so each is
    /// finalized against `base_dir` (typically the working directory): a
    /// relative `skill` resolves there, and an unnamed case defaults to `case`
    /// (suffixed with its index when several are unnamed, keeping report keys
    /// distinct).
    ///
    /// # Errors
    /// [`Error::Invalid`] if the JSON does not parse into test cases, or if any
    /// case is internally inconsistent.
    pub fn from_json(json: &str, base_dir: &Path) -> Result<Vec<Self>> {
        // Accept either a bare object (one case) or an array of them, so a
        // single code-defined case need not be wrapped.
        let value: serde_json::Value = serde_json::from_str(json)
            .map_err(|e| Error::Invalid(format!("invalid --case-json: {e}")))?;
        let mut cases: Vec<TestCase> = match value {
            serde_json::Value::Array(_) => serde_json::from_value(value),
            _ => serde_json::from_value(value).map(|c| vec![c]),
        }
        .map_err(|e| Error::Invalid(format!("invalid --case-json: {e}")))?;
        for (index, case) in cases.iter_mut().enumerate() {
            let default_name = if index == 0 {
                "case".to_string()
            } else {
                format!("case-{}", index + 1)
            };
            case.finalize(base_dir, &default_name)?;
        }
        Ok(cases)
    }

    /// Finalize an in-memory case: default `name` to `default_name` when unset,
    /// resolve a relative `skill` against `base_dir`, and validate. Shared by
    /// [`load`](Self::load) (anchored at the file's directory) and
    /// [`from_json`](Self::from_json) (anchored at the working directory, since
    /// a code-defined case has no file).
    ///
    /// # Errors
    /// [`Error::Invalid`] if the case is internally inconsistent.
    pub fn finalize(&mut self, base_dir: &Path, default_name: &str) -> Result<()> {
        if self.name.is_empty() {
            self.name = default_name.to_string();
        }
        if self.skill.is_relative() {
            self.skill = base_dir.join(&self.skill);
        }
        self.validate()
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
        for (i, decl) in self.mocks.iter().enumerate() {
            let label = decl.name.clone().unwrap_or_else(|| format!("#{i}"));
            decl.validate(&format!("test case `{}`, mock `{label}`", self.name))?;
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
    use crate::eval::{BooleanEval, Eval};

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
        assert!(matches!(case.evals[0], Eval::Boolean(BooleanEval { .. })));
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

    #[test]
    fn unknown_eval_field_is_rejected_not_silently_ignored() {
        // A typo'd eval key (`expcted`) must be a loud parse error naming the
        // field — never a silently-applied default, which could invert the
        // eval's intent into a vacuous pass.
        let yaml = "skill: ./x\ninput: hi\nevals:\n  - type: boolean\n    criterion: c\n    expcted: false\n";
        let err = serde_yaml::from_str::<TestCase>(yaml).unwrap_err();
        assert!(
            err.to_string().contains("expcted"),
            "error names the unknown field: {err}"
        );
        // Same strictness through the JSON path the SDKs use.
        let json =
            r#"{"skill":"./x","input":"hi","evals":[{"type":"called","mock":"m","tmies":1}]}"#;
        let err = TestCase::from_json(json, Path::new(".")).unwrap_err();
        assert!(err.to_string().contains("tmies"), "{err}");
    }

    #[test]
    fn unknown_user_field_is_rejected() {
        let yaml = "skill: ./x\ninput: hi\nuser:\n  persona: p\n  don_when: x\nevals:\n  - type: boolean\n    criterion: c\n";
        let err = serde_yaml::from_str::<TestCase>(yaml).unwrap_err();
        assert!(err.to_string().contains("don_when"), "{err}");
    }

    /// Write `yaml` into a unique temp dir as `name`, returning the file path.
    fn case_file(tag: &str, name: &str, yaml: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "skilltest-case-{}-{tag}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, yaml).unwrap();
        path
    }

    #[test]
    fn load_defaults_name_from_stem_and_resolves_skill_path() {
        let path = case_file(
            "load",
            "greet_pass.yaml",
            "skill: ./greeter\ninput: \"hi\"\nevals:\n  - type: boolean\n    criterion: \"greets\"\n",
        );
        let case = TestCase::load(&path).unwrap();
        assert_eq!(case.name, "greet_pass");
        // The relative skill path is resolved against the case file's directory.
        assert!(case.skill.is_absolute());
        assert!(case.skill.ends_with("greeter"));
    }

    #[test]
    fn load_keeps_explicit_name() {
        let path = case_file(
            "named",
            "x.yaml",
            "name: custom\nskill: ./s\ninput: hi\nevals:\n  - type: boolean\n    criterion: c\n",
        );
        assert_eq!(TestCase::load(&path).unwrap().name, "custom");
    }

    #[test]
    fn load_missing_file_is_io_error() {
        let path =
            std::env::temp_dir().join(format!("skilltest-nocase-{}.yaml", std::process::id()));
        assert!(matches!(TestCase::load(&path), Err(Error::Io { .. })));
    }

    #[test]
    fn load_malformed_yaml_is_yaml_error() {
        let path = case_file("bad", "bad.yaml", "input: [unterminated\n");
        assert!(matches!(TestCase::load(&path), Err(Error::Yaml { .. })));
    }

    #[test]
    fn load_inconsistent_case_is_invalid_error() {
        // Parses, but an empty input fails validation.
        let path = case_file(
            "blank",
            "blank.yaml",
            "skill: ./s\ninput: \"\"\nevals:\n  - type: boolean\n    criterion: c\n",
        );
        assert!(matches!(TestCase::load(&path), Err(Error::Invalid(_))));
    }

    #[test]
    fn validate_rejects_empty_input_and_evals() {
        let mut case = TestCase {
            name: "t".into(),
            skill: PathBuf::from("./s"),
            input: "   ".into(),
            user: None,
            mocks: Vec::new(),
            spy: false,
            evals: vec![Eval::Boolean(BooleanEval {
                criterion: "c".into(),
                expected: true,
                name: None,
            })],
        };
        assert!(case.validate().is_err(), "blank input");
        case.input = "ok".into();
        case.evals.clear();
        assert!(case.validate().is_err(), "no evals");
    }

    #[test]
    fn validate_rejects_blank_persona_and_zero_user_max_turns() {
        let base = TestCase {
            name: "t".into(),
            skill: PathBuf::from("./s"),
            input: "ok".into(),
            user: None,
            mocks: Vec::new(),
            spy: false,
            evals: vec![Eval::Boolean(BooleanEval {
                criterion: "c".into(),
                expected: true,
                name: None,
            })],
        };
        let mut blank_persona = base.clone();
        blank_persona.user = Some(SimulatedUser {
            persona: "  ".into(),
            done_when: None,
            max_turns: None,
        });
        assert!(blank_persona.validate().is_err());

        let mut zero_turns = base;
        zero_turns.user = Some(SimulatedUser {
            persona: "a patient".into(),
            done_when: None,
            max_turns: Some(0),
        });
        assert!(zero_turns.validate().is_err());
    }

    #[test]
    fn from_json_parses_a_single_object_and_resolves_skill() {
        let json = r#"{"skill":"./greeter","input":"hi","evals":[{"type":"boolean","criterion":"greets"}]}"#;
        let base = Path::new("/work/cases");
        let cases = TestCase::from_json(json, base).unwrap();
        assert_eq!(cases.len(), 1);
        assert_eq!(cases[0].name, "case");
        // A relative skill resolves against the supplied base directory.
        assert_eq!(cases[0].skill, PathBuf::from("/work/cases/greeter"));
    }

    #[test]
    fn from_json_parses_an_array_and_indexes_unnamed_cases() {
        let json = r#"[
            {"skill":"/abs/a","input":"hi","evals":[{"type":"boolean","criterion":"c"}]},
            {"name":"named","skill":"/abs/b","input":"hi","evals":[{"type":"boolean","criterion":"c"}]},
            {"skill":"/abs/c","input":"hi","evals":[{"type":"boolean","criterion":"c"}]}
        ]"#;
        let cases = TestCase::from_json(json, Path::new("/work")).unwrap();
        assert_eq!(
            cases.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            ["case", "named", "case-3"]
        );
        // An absolute skill is left untouched.
        assert_eq!(cases[0].skill, PathBuf::from("/abs/a"));
    }

    #[test]
    fn from_json_rejects_malformed_and_inconsistent_cases() {
        // Not JSON at all.
        assert!(matches!(
            TestCase::from_json("{not json", Path::new(".")),
            Err(Error::Invalid(_))
        ));
        // An unknown field is rejected (the case type denies them).
        let unknown = r#"{"skill":"./x","input":"hi","bogus":1,"evals":[{"type":"boolean","criterion":"c"}]}"#;
        assert!(matches!(
            TestCase::from_json(unknown, Path::new(".")),
            Err(Error::Invalid(_))
        ));
        // Parses, but validation fails (no evals).
        let empty_evals = r#"{"skill":"./x","input":"hi","evals":[]}"#;
        assert!(matches!(
            TestCase::from_json(empty_evals, Path::new(".")),
            Err(Error::Invalid(_))
        ));
    }

    #[test]
    fn discover_cases_returns_single_file() {
        let path = case_file("one", "only.yaml", "skill: ./s\ninput: hi\nevals: []\n");
        let found = discover_cases(&path).unwrap();
        assert_eq!(found, vec![path]);
    }

    #[test]
    fn discover_cases_lists_yaml_in_a_directory_sorted() {
        let first = case_file("dir", "b.yaml", "skill: ./s\ninput: hi\nevals: []\n");
        let dir = first.parent().unwrap().to_path_buf();
        std::fs::write(dir.join("a.yml"), "skill: ./s\ninput: hi\nevals: []\n").unwrap();
        // A non-YAML file is ignored.
        std::fs::write(dir.join("notes.txt"), "ignore me").unwrap();
        let found = discover_cases(&dir).unwrap();
        assert_eq!(found.len(), 2);
        // Sorted: a.yml before b.yaml.
        assert!(found[0].ends_with("a.yml"));
        assert!(found[1].ends_with("b.yaml"));
    }

    #[test]
    fn discover_cases_empty_directory_is_invalid() {
        let dir = std::env::temp_dir().join(format!("skilltest-emptycases-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(matches!(discover_cases(&dir), Err(Error::Invalid(_))));
    }

    #[test]
    fn discover_cases_missing_path_is_invalid() {
        let path = std::env::temp_dir().join(format!("skilltest-nopath-{}", std::process::id()));
        assert!(matches!(discover_cases(&path), Err(Error::Invalid(_))));
    }
}
