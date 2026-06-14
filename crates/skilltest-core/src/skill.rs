//! Skill definitions: a directory containing a `SKILL.md` with YAML frontmatter
//! and a Markdown body. This module loads them and validates them, powering the
//! `skilltest validate` subcommand.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{Error, Result};

/// The YAML frontmatter at the top of a `SKILL.md`. Only `name` and
/// `description` are required; unknown keys are allowed so authors can carry
/// extra metadata.
#[derive(Debug, Clone, Deserialize)]
pub struct Frontmatter {
    pub name: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
}

/// A loaded skill: where it lives, its parsed frontmatter, and its instruction
/// body (everything after the frontmatter), which is what we hand to a provider
/// when running the skill.
#[derive(Debug, Clone)]
pub struct SkillDefinition {
    pub dir: PathBuf,
    pub name: String,
    pub description: String,
    pub instructions: String,
}

/// A single validation problem found in a skill definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub skill: PathBuf,
    pub message: String,
}

impl Finding {
    fn new(skill: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Self {
            skill: skill.into(),
            message: message.into(),
        }
    }
}

/// Split a `SKILL.md` into `(frontmatter_yaml, body)`. Returns `None` for the
/// frontmatter when the document does not open with a `---` fence.
fn split_frontmatter(text: &str) -> (Option<&str>, &str) {
    let rest = match text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    {
        Some(rest) => rest,
        None => return (None, text),
    };
    // Find the closing fence at the start of a line.
    for sep in ["\n---\n", "\n---\r\n", "\r\n---\r\n"] {
        if let Some(idx) = rest.find(sep) {
            let fm = &rest[..idx];
            let body = &rest[idx + sep.len()..];
            return (Some(fm), body);
        }
    }
    // Opened a fence but never closed it.
    (None, text)
}

/// Load a skill definition from a directory containing `SKILL.md`.
///
/// # Errors
/// Returns [`Error::Io`] if `SKILL.md` cannot be read and [`Error::Yaml`] if the
/// frontmatter is not valid YAML.
pub fn load_skill(dir: &Path) -> Result<SkillDefinition> {
    let skill_md = dir.join("SKILL.md");
    let text = std::fs::read_to_string(&skill_md).map_err(|source| Error::Io {
        path: skill_md.clone(),
        source,
    })?;
    let (fm, body) = split_frontmatter(&text);
    let frontmatter: Frontmatter = match fm {
        Some(fm) => serde_yaml::from_str(fm).map_err(|source| Error::Yaml {
            path: skill_md.clone(),
            source,
        })?,
        None => Frontmatter {
            name: None,
            description: None,
            license: None,
        },
    };
    Ok(SkillDefinition {
        dir: dir.to_path_buf(),
        name: frontmatter.name.unwrap_or_default(),
        description: frontmatter.description.unwrap_or_default(),
        instructions: body.trim().to_string(),
    })
}

/// Validate a single skill directory, returning any findings. An empty vec
/// means the skill is valid. This never errors on a *bad* skill — invalidity is
/// reported as findings; it only errors if the directory is unreadable.
///
/// # Errors
/// Returns [`Error::Io`] only when the directory exists but cannot be inspected
/// in a way that prevents validation from proceeding.
pub fn validate_skill(dir: &Path) -> Result<Vec<Finding>> {
    let skill_md = dir.join("SKILL.md");
    if !skill_md.is_file() {
        return Ok(vec![Finding::new(
            dir,
            "missing SKILL.md (a skill is a directory containing SKILL.md)",
        )]);
    }

    let text = std::fs::read_to_string(&skill_md).map_err(|source| Error::Io {
        path: skill_md.clone(),
        source,
    })?;

    let mut findings = Vec::new();
    let (fm, body) = split_frontmatter(&text);

    let Some(fm) = fm else {
        findings.push(Finding::new(
            dir,
            "SKILL.md has no YAML frontmatter (expected a leading `---` fenced block)",
        ));
        return Ok(findings);
    };

    match serde_yaml::from_str::<Frontmatter>(fm) {
        Ok(frontmatter) => {
            match frontmatter.name.as_deref().map(str::trim) {
                None | Some("") => findings.push(Finding::new(
                    dir,
                    "frontmatter is missing a non-empty `name`",
                )),
                Some(name) => {
                    if let Some(folder) = dir.file_name().and_then(|s| s.to_str()) {
                        if folder != name {
                            findings.push(Finding::new(
                                dir,
                                format!(
                                    "frontmatter `name` ({name}) does not match the directory name ({folder})"
                                ),
                            ));
                        }
                    }
                }
            }
            match frontmatter.description.as_deref().map(str::trim) {
                None | Some("") => findings.push(Finding::new(
                    dir,
                    "frontmatter is missing a non-empty `description`",
                )),
                Some(desc) if desc.len() < 16 => findings.push(Finding::new(
                    dir,
                    "frontmatter `description` is too short to be useful (< 16 chars)",
                )),
                Some(_) => {}
            }
        }
        Err(source) => {
            findings.push(Finding::new(
                dir,
                format!("frontmatter is not valid YAML: {source}"),
            ));
        }
    }

    if body.trim().is_empty() {
        findings.push(Finding::new(
            dir,
            "SKILL.md has no instruction body after the frontmatter",
        ));
    }

    Ok(findings)
}

/// Validate either a single skill directory or a folder *containing* skill
/// directories. A directory is treated as a skill if it directly contains a
/// `SKILL.md`; otherwise its immediate subdirectories are validated.
///
/// # Errors
/// Propagates [`Error::Io`] from reading the directory tree.
pub fn validate_path(path: &Path) -> Result<Vec<Finding>> {
    if path.join("SKILL.md").is_file() {
        return validate_skill(path);
    }

    let entries = std::fs::read_dir(path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;

    let mut skill_dirs: Vec<PathBuf> = entries
        .filter_map(std::result::Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.join("SKILL.md").is_file())
        .collect();
    skill_dirs.sort();

    if skill_dirs.is_empty() {
        return Ok(vec![Finding::new(
            path,
            "no skills found (expected a SKILL.md here or in an immediate subdirectory)",
        )]);
    }

    let mut findings = Vec::new();
    for dir in skill_dirs {
        findings.extend(validate_skill(&dir)?);
    }
    Ok(findings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_frontmatter_and_body() {
        let text = "---\nname: greeter\ndescription: hi\n---\nBody here\n";
        let (fm, body) = split_frontmatter(text);
        assert_eq!(fm, Some("name: greeter\ndescription: hi"));
        assert_eq!(body, "Body here\n");
    }

    #[test]
    fn no_frontmatter_returns_none() {
        let (fm, body) = split_frontmatter("# Just a heading\n");
        assert!(fm.is_none());
        assert_eq!(body, "# Just a heading\n");
    }

    #[test]
    fn unclosed_fence_yields_no_frontmatter() {
        // Opening `---` but no closing fence falls back to "no frontmatter".
        let (fm, body) = split_frontmatter("---\nname: x\nstill going\n");
        assert!(fm.is_none());
        assert_eq!(body, "---\nname: x\nstill going\n");
    }

    /// Make a unique temp skill directory with a `SKILL.md` of `contents`.
    fn skill_dir(tag: &str, name: &str, contents: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "skilltest-skill-{}-{tag}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), contents).unwrap();
        dir
    }

    #[test]
    fn load_skill_reads_frontmatter_and_body() {
        let dir = skill_dir(
            "load",
            "greeter",
            "---\nname: greeter\ndescription: a friendly greeter\n---\nGreet warmly.\n",
        );
        let skill = load_skill(&dir).unwrap();
        assert_eq!(skill.name, "greeter");
        assert_eq!(skill.description, "a friendly greeter");
        assert_eq!(skill.instructions, "Greet warmly.");
    }

    #[test]
    fn load_skill_without_frontmatter_uses_defaults() {
        let dir = skill_dir("nofm", "bare", "# Just a body\nNo frontmatter here.\n");
        let skill = load_skill(&dir).unwrap();
        assert_eq!(skill.name, "");
        assert_eq!(skill.description, "");
        assert!(skill.instructions.contains("No frontmatter here."));
    }

    #[test]
    fn load_skill_missing_file_is_io_error() {
        let dir = std::env::temp_dir().join(format!("skilltest-missing-{}", std::process::id()));
        assert!(matches!(load_skill(&dir), Err(Error::Io { .. })));
    }

    #[test]
    fn load_skill_bad_frontmatter_is_yaml_error() {
        let dir = skill_dir("badyaml", "x", "---\nname: [unterminated\n---\nbody\n");
        assert!(matches!(load_skill(&dir), Err(Error::Yaml { .. })));
    }

    #[test]
    fn validate_skill_accepts_a_good_skill() {
        let dir = skill_dir(
            "good",
            "greeter",
            "---\nname: greeter\ndescription: a sufficiently long description\n---\nDo the thing.\n",
        );
        assert!(validate_skill(&dir).unwrap().is_empty());
    }

    #[test]
    fn validate_skill_flags_missing_skill_md() {
        let root = std::env::temp_dir().join(format!("skilltest-empty-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let findings = validate_skill(&root).unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("missing SKILL.md"));
    }

    #[test]
    fn validate_skill_flags_missing_frontmatter() {
        let dir = skill_dir("nofm2", "x", "Just a body, no fence.\n");
        let findings = validate_skill(&dir).unwrap();
        assert!(findings
            .iter()
            .any(|f| f.message.contains("no YAML frontmatter")));
    }

    #[test]
    fn validate_skill_flags_name_mismatch_and_short_description() {
        let dir = skill_dir(
            "mismatch",
            "actual-folder",
            "---\nname: declared-name\ndescription: short\n---\nbody\n",
        );
        let findings = validate_skill(&dir).unwrap();
        assert!(findings
            .iter()
            .any(|f| f.message.contains("does not match the directory name")));
        assert!(findings.iter().any(|f| f.message.contains("too short")));
    }

    #[test]
    fn validate_skill_flags_missing_name_description_and_body() {
        let dir = skill_dir("blank", "blank", "---\nlicense: MIT\n---\n\n");
        let findings = validate_skill(&dir).unwrap();
        assert!(findings
            .iter()
            .any(|f| f.message.contains("non-empty `name`")));
        assert!(findings
            .iter()
            .any(|f| f.message.contains("non-empty `description`")));
        assert!(findings
            .iter()
            .any(|f| f.message.contains("no instruction body")));
    }

    #[test]
    fn validate_skill_flags_invalid_frontmatter_yaml() {
        let dir = skill_dir("invalidyaml", "x", "---\nname: [unterminated\n---\nbody\n");
        let findings = validate_skill(&dir).unwrap();
        assert!(findings
            .iter()
            .any(|f| f.message.contains("not valid YAML")));
    }

    #[test]
    fn validate_path_on_a_single_skill_dir() {
        let dir = skill_dir(
            "single",
            "greeter",
            "---\nname: greeter\ndescription: a long enough description\n---\nbody\n",
        );
        assert!(validate_path(&dir).unwrap().is_empty());
    }

    #[test]
    fn validate_path_over_a_folder_of_skills() {
        // Build a parent dir holding two skill subdirs (one good, one bad).
        let good = skill_dir(
            "folder-good",
            "greeter",
            "---\nname: greeter\ndescription: a long enough description\n---\nbody\n",
        );
        let parent = good.parent().unwrap().to_path_buf();
        let bad = parent.join("broken");
        std::fs::create_dir_all(&bad).unwrap();
        std::fs::write(bad.join("SKILL.md"), "no frontmatter at all\n").unwrap();
        let findings = validate_path(&parent).unwrap();
        // The bad skill contributes a finding; the good one doesn't.
        assert!(findings
            .iter()
            .any(|f| f.message.contains("no YAML frontmatter")));
    }

    #[test]
    fn validate_path_reports_empty_folder() {
        let root = std::env::temp_dir().join(format!("skilltest-noskills-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let findings = validate_path(&root).unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("no skills found"));
    }

    #[test]
    fn validate_path_missing_dir_is_io_error() {
        let dir = std::env::temp_dir().join(format!("skilltest-nodir-{}", std::process::id()));
        assert!(matches!(validate_path(&dir), Err(Error::Io { .. })));
    }
}
