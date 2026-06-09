//! Scaffolding for `skilltest init`: write a runnable starter project (config,
//! an example skill, and an example case) into a directory, never clobbering
//! existing files.

use std::path::{Path, PathBuf};

use skilltest_core::{Error, Result};

const CONFIG: &str = "\
# skilltest configuration. See https://github.com/nickderobertis/skilltest.
# The default provider runs skills through `oneharness` (must be on PATH).
provider:
  kind: oneharness
  bin: oneharness
  judge_harness: claude-code
platforms:
  - claude-code
models:
  - sonnet
judge_model: haiku
max_turns: 8
";

const SKILL: &str = "\
---
name: example
description: An example greeter skill scaffolded by `skilltest init`.
---
# Example greeter

Greet the user by name in one warm, professional sentence. Always use their
title and surname.

<!-- Offline demo only: skilltest-fake-provider replies with this line so you can
     try `skilltest run` without a real provider. A real model ignores it.
     fake-reply: Hello, Dr. Smith! Your appointment is confirmed. -->
";

const CASE: &str = "\
# An example test case. Run it with:
#   skilltest run cases/example.yaml
# or try it offline against the bundled fake provider:
#   skilltest run cases/example.yaml --provider skilltest-fake-provider
name: example
skill: ../skills/example
input: \"Greet Dr. Smith, who has an appointment today.\"
evals:
  - type: boolean
    name: names-the-patient
    criterion: \"the reply greets `Dr. Smith` by name\"
  - type: numeric
    name: warmth
    criterion: \"how warm and professional is the greeting\"
    min: 0
    max: 10
    threshold: 7
";

/// The files `init` lays down, relative to the target directory.
const FILES: [(&str, &str); 3] = [
    ("skilltest.yaml", CONFIG),
    ("skills/example/SKILL.md", SKILL),
    ("cases/example.yaml", CASE),
];

/// Write the starter project into `dir`. Refuses to run if any target file
/// already exists, so it never overwrites a user's work.
///
/// # Errors
/// [`Error::Invalid`] if a target file already exists; [`Error::Io`] if a file
/// or directory cannot be created.
pub fn scaffold(dir: &Path) -> Result<Vec<PathBuf>> {
    // Check everything first so a partial scaffold never lands on a conflict.
    for (rel, _) in FILES {
        let path = dir.join(rel);
        if path.exists() {
            return Err(Error::Invalid(format!(
                "refusing to overwrite existing {}; run `skilltest init` in an empty directory",
                path.display()
            )));
        }
    }

    let mut created = Vec::with_capacity(FILES.len());
    for (rel, content) in FILES {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| Error::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        std::fs::write(&path, content).map_err(|source| Error::Io {
            path: path.clone(),
            source,
        })?;
        created.push(path);
    }
    Ok(created)
}
