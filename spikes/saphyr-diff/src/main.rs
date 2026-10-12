//! Differential harness: serde_yaml 0.9 (today) vs serde-saphyr 1.3 (and
//! saphyr 0.2's node tree) over this repository's real YAML.
//!
//! The "current" side calls skilltest-core's real loaders (`Config::load`,
//! `TestCase::load`, `load_skill`, `validate_skill`). The serde-saphyr side
//! replays the same steps with the parse call swapped; each replica is first
//! run with serde_yaml and checked against the real loader, so a difference
//! the harness reports is the crate's, not the replica's.
//!
//! Prints every difference per document kind; exits 1 only if a replica
//! self-check fails.

use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use serde_saphyr::{DuplicateKeyPolicy, MergeKeyPolicy, Options, SerializerOptions};
use skilltest_core::skill::{load_skill, validate_skill, Frontmatter, SkillDefinition};
use skilltest_core::{
    ApiJudgeConfig, ApiVendor, Config, Finding, JudgeConfig, MockDecl, TestCase,
};

// ---------------------------------------------------------------------------
// Engines
// ---------------------------------------------------------------------------

#[derive(Clone)]
enum Engine {
    SerdeYaml,
    Saphyr(&'static str, Options),
}

impl Engine {
    fn parse<T: DeserializeOwned>(&self, text: &str) -> Result<T, String> {
        match self {
            Engine::SerdeYaml => serde_yaml::from_str(text).map_err(|e| e.to_string()),
            Engine::Saphyr(_, o) => {
                serde_saphyr::from_str_with_options(text, o.clone()).map_err(|e| e.to_string())
            }
        }
    }
}

fn saphyr_default() -> Engine {
    Engine::Saphyr("default", Options::default())
}

/// Option presets tried against each difference the defaults show.
fn presets() -> Vec<Engine> {
    let mut v = Vec::new();
    let mut o = Options::default();
    o.strict_booleans = true;
    v.push(Engine::Saphyr("strict_booleans", o));
    let mut o = Options::default();
    o.no_schema = true;
    v.push(Engine::Saphyr("no_schema", o));
    let mut o = Options::default();
    o.strict_booleans = true;
    o.no_schema = true;
    v.push(Engine::Saphyr("strict_booleans+no_schema", o));
    let mut o = Options::default();
    o.duplicate_keys = DuplicateKeyPolicy::LastWins;
    v.push(Engine::Saphyr("duplicate_keys=LastWins", o));
    let mut o = Options::default();
    o.duplicate_keys = DuplicateKeyPolicy::FirstWins;
    v.push(Engine::Saphyr("duplicate_keys=FirstWins", o));
    let mut o = Options::default();
    o.merge_keys = MergeKeyPolicy::AsOrdinary;
    v.push(Engine::Saphyr("merge_keys=AsOrdinary", o));
    let mut o = Options::default();
    o.merge_keys = MergeKeyPolicy::Error;
    v.push(Engine::Saphyr("merge_keys=Error", o));
    let mut o = Options::default();
    o.legacy_octal_numbers = true;
    v.push(Engine::Saphyr("legacy_octal_numbers", o));
    let mut o = Options::default();
    o.reject_non_finite_typeless_float = false;
    v.push(Engine::Saphyr("reject_non_finite_typeless_float=false", o));
    let mut o = Options::default();
    o.with_snippet = false;
    v.push(Engine::Saphyr("with_snippet=false", o));
    let mut o = Options::default();
    o.strict_booleans = true;
    o.with_snippet = false;
    v.push(Engine::Saphyr("strict_booleans+with_snippet=false", o));
    v
}

// ---------------------------------------------------------------------------
// Documents
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Config,
    Case,
    Skill,
    Mocks,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::Config => "config (Config::load)",
            Kind::Case => "test case (TestCase::load)",
            Kind::Skill => "SKILL.md (load_skill / validate_skill)",
            Kind::Mocks => "--mocks file (cli load_mocks_file)",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Origin {
    /// A committed document of this repository (or an SDK-written mocks file).
    Real,
    /// A spike fixture covering variants no real document uses.
    Variant,
    /// A synthetic probe of input a user could write.
    Probe,
    /// One of the error cases the task names.
    Error,
    /// A real document of another repository (`--external <dir>`, read-only).
    External,
}

struct Doc {
    kind: Kind,
    origin: Origin,
    label: String,
    /// The file (for a skill, the skill directory).
    path: PathBuf,
}

impl Doc {
    /// The YAML text the crates parse (a skill's frontmatter only).
    fn yaml(&self) -> Option<String> {
        if self.kind == Kind::Skill {
            let text = std::fs::read_to_string(self.path.join("SKILL.md")).ok()?;
            split_frontmatter(&text).0.map(str::to_string)
        } else {
            std::fs::read_to_string(&self.path).ok()
        }
    }
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn spike_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn real_docs(root: &Path) -> Vec<Doc> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("ls-files")
        .output()
        .expect("git ls-files");
    let mut docs = Vec::new();
    for line in String::from_utf8(out.stdout).unwrap().lines() {
        if line.starts_with("spikes/") || line.starts_with('.') {
            continue;
        }
        let path = root.join(line);
        if line.ends_with("/SKILL.md") {
            docs.push(Doc {
                kind: Kind::Skill,
                origin: Origin::Real,
                label: line.to_string(),
                path: path.parent().unwrap().to_path_buf(),
            });
        } else if line == "examples/skilltest.yaml" {
            docs.push(Doc {
                kind: Kind::Config,
                origin: Origin::Real,
                label: line.to_string(),
                path,
            });
        } else if (line.ends_with(".yaml") || line.ends_with(".yml"))
            && ["tests/", "screenshots/", "plugins/"]
                .iter()
                .any(|dir| line.starts_with(dir))
        {
            docs.push(Doc {
                kind: Kind::Case,
                origin: Origin::Real,
                label: line.to_string(),
                path,
            });
        }
    }
    docs
}

fn spike_docs() -> Vec<Doc> {
    let dir = spike_dir().join("fixtures");
    let mut docs = Vec::new();
    for name in [
        "python.json",
        "typescript.json",
        "python-astral.json",
        "typescript-astral.json",
    ] {
        docs.push(Doc {
            kind: Kind::Mocks,
            origin: Origin::Real,
            label: format!("sdk-mocks/{name} (SDK-written)"),
            path: dir.join("sdk-mocks").join(name),
        });
    }
    let mut variants: Vec<_> = std::fs::read_dir(dir.join("variants"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    variants.sort();
    for path in variants {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let kind = if name.starts_with("config_") {
            Kind::Config
        } else {
            Kind::Case
        };
        docs.push(Doc {
            kind,
            origin: Origin::Variant,
            label: format!("variants/{name}"),
            path,
        });
    }
    docs
}

/// Real documents of another checkout, read-only: every SKILL.md, every
/// `skilltest.yaml` (config), and every other YAML file whose top level has
/// both `skill` and `evals` (a test case).
fn external_docs(dir: &Path) -> Vec<Doc> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for e in entries.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if p.is_dir() {
                if !["node_modules", "target", ".git", ".venv", "dist"].contains(&name.as_str()) {
                    walk(&p, out);
                }
            } else {
                out.push(p);
            }
        }
    }
    let mut files = Vec::new();
    walk(dir, &mut files);
    files.sort();
    let mut docs = Vec::new();
    for path in files {
        let rel = path.strip_prefix(dir).unwrap_or(&path).display().to_string();
        let label = format!("{}/{rel}", dir.file_name().map_or(String::new(), |n| n.to_string_lossy().to_string()));
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let kind = if name == "SKILL.md" {
            Some(Kind::Skill)
        } else if name == "skilltest.yaml" {
            Some(Kind::Config)
        } else if name.ends_with(".yaml") || name.ends_with(".yml") {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let is_case = ["skill", "evals"].iter().all(|k| {
                text.lines().any(|l| l.starts_with(&format!("{k}:")))
            });
            is_case.then_some(Kind::Case)
        } else {
            None
        };
        if let Some(kind) = kind {
            let path = if kind == Kind::Skill { path.parent().unwrap().to_path_buf() } else { path };
            docs.push(Doc { kind, origin: Origin::External, label, path });
        }
    }
    docs
}

const CASE_BASE: &str = "skill: ./x\ninput: hi\nevals:\n  - type: boolean\n    criterion: c\n";

fn case_with(field: &str) -> String {
    format!("skill: ./x\n{field}\nevals:\n  - type: boolean\n    criterion: c\n")
}

fn case_mock(mock: &str) -> String {
    format!("{CASE_BASE}mocks:\n{mock}")
}

/// Synthetic probes and the named error cases, written into `dir`.
fn probe_docs(dir: &Path) -> Vec<Doc> {
    use Kind::*;
    use Origin::*;
    let probes: Vec<(Origin, Kind, &str, String)> = vec![
        // --- error cases the task names ---
        (Error, Case, "expcted", "skill: ./x\ninput: hi\nevals:\n  - type: boolean\n    criterion: c\n    expcted: false\n".into()),
        (Error, Case, "don_when", "skill: ./x\ninput: hi\nuser:\n  persona: p\n  don_when: x\nevals:\n  - type: boolean\n    criterion: c\n".into()),
        (Error, Case, "unterminated", "input: [unterminated\n".into()),
        (Error, Config, "unterminated", "platforms: [unterminated\n".into()),
        (Error, Mocks, "unterminated", "[unterminated\n".into()),
        (Error, Skill, "unterminated", "---\nname: [unterminated\n---\nbody\n".into()),
        (Error, Case, "bogus-top-level", "skill: ./x\ninput: hi\nbogus: 1\nevals: []\n".into()),
        (Error, Case, "stubb-in-mock", case_mock("  - match: { tool: bash }\n    stubb: x\n")),
        (Error, Config, "bogus-top-level", "bogus: 1\n".into()),
        (Error, Config, "provider-unknown-field", "provider:\n  kind: oneharness\n  bnn: x\n".into()),
        (Error, Mocks, "not-a-list", "{\"match\": {\"tool\": \"bash\"}}\n".into()),
        (Error, Skill, "colon-in-plain-description", "---\nname: unterminated\ndescription: Use this skill: when deploying things\n---\nbody\n".into()),
        // --- scalar typing into typed fields ---
        (Probe, Case, "input: yes", case_with("input: yes")),
        (Probe, Case, "input: 42", case_with("input: 42")),
        (Probe, Case, "input: 1.10", case_with("input: 1.10")),
        (Probe, Case, "input: true", case_with("input: true")),
        (Probe, Case, "input: ~", case_with("input: ~")),
        (Probe, Case, "input: 0x1F", case_with("input: 0x1F")),
        (Probe, Case, "input: 017", case_with("input: 017")),
        (Probe, Case, "input: 2026-10-12", case_with("input: 2026-10-12")),
        (Probe, Case, "input: !!str 42", case_with("input: !!str 42")),
        (Probe, Case, "expected: yes", "skill: ./x\ninput: hi\nevals:\n  - type: boolean\n    criterion: c\n    expected: yes\n".into()),
        (Probe, Case, "expected: True", "skill: ./x\ninput: hi\nevals:\n  - type: boolean\n    criterion: c\n    expected: True\n".into()),
        (Probe, Case, "expected: \"false\"", "skill: ./x\ninput: hi\nevals:\n  - type: boolean\n    criterion: c\n    expected: \"false\"\n".into()),
        (Probe, Case, "max_turns: 0x10", "skill: ./x\ninput: hi\nuser:\n  persona: p\n  max_turns: 0x10\nevals:\n  - type: boolean\n    criterion: c\n".into()),
        (Probe, Case, "max_turns: 010", "skill: ./x\ninput: hi\nuser:\n  persona: p\n  max_turns: 010\nevals:\n  - type: boolean\n    criterion: c\n".into()),
        (Probe, Case, "max_turns: 1_0", "skill: ./x\ninput: hi\nuser:\n  persona: p\n  max_turns: 1_0\nevals:\n  - type: boolean\n    criterion: c\n".into()),
        (Probe, Case, "max_turns: +8", "skill: ./x\ninput: hi\nuser:\n  persona: p\n  max_turns: +8\nevals:\n  - type: boolean\n    criterion: c\n".into()),
        (Probe, Case, "max_turns: \"4\"", "skill: ./x\ninput: hi\nuser:\n  persona: p\n  max_turns: \"4\"\nevals:\n  - type: boolean\n    criterion: c\n".into()),
        (Probe, Case, "times: 1.0", "skill: ./x\ninput: hi\nmocks:\n  - name: m\n    match: { tool: bash }\nevals:\n  - type: called\n    mock: m\n    times: 1.0\n".into()),
        (Probe, Case, "threshold: 7 (int into f64)", "skill: ./x\ninput: hi\nevals:\n  - type: numeric\n    criterion: c\n    min: 0\n    max: 10\n    threshold: 7\n".into()),
        (Probe, Case, "stub: true", case_mock("  - match: { tool: bash }\n    stub: true\n")),
        (Probe, Case, "stub: 42", case_mock("  - match: { tool: bash }\n    stub: 42\n")),
        (Probe, Case, "deny: ~", case_mock("  - match: { tool: bash }\n    deny: ~\n")),
        (Probe, Case, "where bare int", "skill: ./x\ninput: hi\nmocks:\n  - name: m\n    match: { tool: bash }\nevals:\n  - type: called\n    mock: m\n    where: { count: 3 }\n".into()),
        (Probe, Case, "duplicate key", "skill: ./x\ninput: first\ninput: second\nevals:\n  - type: boolean\n    criterion: c\n".into()),
        (Probe, Case, "anchor + merge key", "skill: ./x\ninput: hi\nevals:\n  - &base\n    type: boolean\n    criterion: c\n  - <<: *base\n    criterion: d\n".into()),
        (Probe, Case, "alias reuse", "skill: ./x\ninput: &i hi\nuser:\n  persona: *i\nevals:\n  - type: boolean\n    criterion: *i\n".into()),
        (Probe, Case, "BOM prefix", format!("\u{feff}{CASE_BASE}")),
        (Probe, Case, "CRLF line endings", CASE_BASE.replace('\n', "\r\n")),
        (Probe, Case, "multi-document", format!("{CASE_BASE}---\n{CASE_BASE}")),
        (Probe, Case, "explicit --- and ...", format!("---\n{CASE_BASE}...\n")),
        (Probe, Case, "tab indentation", "skill: ./x\ninput: hi\nevals:\n\t- type: boolean\n\t  criterion: c\n".into()),
        (Probe, Case, "rewrite block scalars", case_mock("  - match: { tool: read }\n    rewrite:\n      lit: |\n        a\n        b\n      fold: >-\n        c\n        d\n")),
        (Probe, Config, "empty file", String::new()),
        (Probe, Config, "comments only", "# nothing configured yet\n".into()),
        (Probe, Config, "provider: {kind: oneharness}", "provider: { kind: oneharness }\n".into()),
        (Probe, Config, "max_turns: 010", "max_turns: 010\n".into()),
        (Probe, Config, "timeout_secs: 1e3", "provider:\n  kind: oneharness\n  timeout_secs: 1e3\n".into()),
        (Probe, Config, "platforms scalar", "platforms: claude-code\n".into()),
        (Probe, Config, "spy: yes", "spy: yes\n".into()),
        (Probe, Config, "judge_model: null", "judge_model: null\n".into()),
        (Probe, Config, "judge_model: (empty)", "judge_model:\nmax_turns: 3\n".into()),
        (Probe, Case, "input: (empty)", "skill: ./x\ninput:\nevals:\n  - type: boolean\n    criterion: c\n".into()),
        (Probe, Case, "criterion: ~", "skill: ./x\ninput: hi\nevals:\n  - type: boolean\n    criterion: ~\n".into()),
        (Probe, Mocks, "empty list", "[]\n".into()),
        (Probe, Mocks, "YAML list", "- match: { tool: bash }\n  deny: no\n".into()),
        (Probe, Skill, "name: 123", "---\nname: 123\ndescription: a description long enough\n---\nbody\n".into()),
        (Probe, Skill, "description: yes", "---\nname: x\ndescription: yes\n---\nbody\n".into()),
        (Probe, Skill, "extra keys", "---\nname: x\ndescription: a description long enough\nmetadata: { a: 1, tags: [x, y] }\nallowed-tools: [Bash, Read]\n---\nbody\n".into()),
        (Probe, Skill, "duplicate name", "---\nname: x\nname: y\ndescription: a description long enough\n---\nbody\n".into()),
        (Probe, Skill, "comments-only frontmatter", "---\n# todo\n---\nbody\n".into()),
        (Probe, Skill, "license: ~", "---\nname: x\ndescription: a description long enough\nlicense: ~\n---\nbody\n".into()),
        (Probe, Skill, "description: > as last key", "---\nname: x\ndescription: >\n  A long\n  folded description.\n---\nbody\n".into()),
        (Probe, Skill, "description: | as last key", "---\nname: x\ndescription: |\n  A long\n  literal description.\n---\nbody\n".into()),
        (Probe, Skill, "description: >- as last key", "---\nname: x\ndescription: >-\n  A long\n  stripped description.\n---\nbody\n".into()),
        (Probe, Skill, "description: > then another key", "---\nname: x\ndescription: >\n  A long\n  folded description.\nlicense: MIT\n---\nbody\n".into()),
        (Probe, Skill, "description: (empty)", "---\nname: x\ndescription:\n---\nbody\n".into()),
        (Probe, Skill, "CRLF frontmatter", "---\r\nname: x\r\ndescription: a description long enough\r\n---\r\nbody\r\n".into()),
    ];
    // One probe per scalar spelling inside `rewrite` (a serde_json::Value), so
    // each spelling's typing is measured on its own.
    let mut probes = probes;
    for scalar in [
        "010", "0o17", "0x1F", "yes", "True", "off", "y", ".inf", ".nan", "2026-01-01",
        "~", "", "18446744073709551616", "1e3", "'010'", "-0", "1_000", "!!str 12",
        "!!int '7'", "!custom x",
    ] {
        probes.push((
            Probe,
            Case,
            Box::leak(format!("rewrite value `{scalar}`").into_boxed_str()),
            case_mock(&format!("  - match: {{ tool: read }}\n    rewrite: {{ v: {scalar} }}\n")),
        ));
    }
    for key in ["1", "true", "null", "~", "[a, b]"] {
        probes.push((
            Probe,
            Case,
            Box::leak(format!("rewrite key `{key}`").into_boxed_str()),
            case_mock(&format!("  - match: {{ tool: read }}\n    rewrite: {{ {key}: v }}\n")),
        ));
    }
    let mut docs = Vec::new();
    for (i, (origin, kind, label, text)) in probes.into_iter().enumerate() {
        let path = if kind == Skill {
            let d = dir.join(format!("p{i}"));
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("SKILL.md"), text).unwrap();
            d
        } else {
            let p = dir.join(format!("p{i}.yaml"));
            std::fs::write(&p, text).unwrap();
            p
        };
        docs.push(Doc {
            kind,
            origin,
            label: label.to_string(),
            path,
        });
    }
    docs
}

// ---------------------------------------------------------------------------
// Replicas of the loaders, parameterised by engine
// ---------------------------------------------------------------------------

fn split_frontmatter(text: &str) -> (Option<&str>, &str) {
    let rest = match text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    {
        Some(rest) => rest,
        None => return (None, text),
    };
    for sep in ["\n---\n", "\n---\r\n", "\r\n---\r\n"] {
        if let Some(idx) = rest.find(sep) {
            return (Some(&rest[..idx]), &rest[idx + sep.len()..]);
        }
    }
    (None, text)
}

fn config_load(e: &Engine, path: &Path) -> Result<Config, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let config: Config = e
        .parse(&text)
        .map_err(|s| format!("invalid YAML in {}: {s}", path.display()))?;
    config.validate().map_err(|e| e.to_string())?;
    Ok(config)
}

fn case_load(e: &Engine, path: &Path) -> Result<TestCase, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut case: TestCase = e
        .parse(&text)
        .map_err(|s| format!("invalid YAML in {}: {s}", path.display()))?;
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("case");
    let base = path.parent().unwrap_or_else(|| Path::new(""));
    case.finalize(base, stem).map_err(|e| e.to_string())?;
    Ok(case)
}

fn mocks_load(e: &Engine, path: &Path) -> Result<Vec<MockDecl>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    e.parse(&text).map_err(|s| {
        format!(
            "invalid test definition: --mocks file `{}` is not a valid mock declaration list: {s}",
            path.display()
        )
    })
}

fn skill_load(e: &Engine, dir: &Path) -> Result<SkillDefinition, String> {
    let skill_md = dir.join("SKILL.md");
    let text = std::fs::read_to_string(&skill_md).map_err(|e| e.to_string())?;
    let (fm, body) = split_frontmatter(&text);
    let fm: Frontmatter = match fm {
        Some(fm) => e
            .parse(fm)
            .map_err(|s| format!("invalid YAML in {}: {s}", skill_md.display()))?,
        None => Frontmatter {
            name: None,
            description: None,
            license: None,
        },
    };
    Ok(SkillDefinition {
        dir: dir.to_path_buf(),
        name: fm.name.unwrap_or_default(),
        description: fm.description.unwrap_or_default(),
        instructions: body.trim().to_string(),
    })
}

fn skill_validate(e: &Engine, dir: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(dir.join("SKILL.md")).unwrap();
    let mut findings = Vec::new();
    let (fm, body) = split_frontmatter(&text);
    let Some(fm) = fm else {
        return vec![
            "SKILL.md has no YAML frontmatter (expected a leading `---` fenced block)".into(),
        ];
    };
    match e.parse::<Frontmatter>(fm) {
        Ok(f) => {
            match f.name.as_deref().map(str::trim) {
                None | Some("") => findings.push("frontmatter is missing a non-empty `name`".into()),
                Some(name) => {
                    if let Some(folder) = dir.file_name().and_then(|s| s.to_str()) {
                        if folder != name {
                            findings.push(format!("frontmatter `name` ({name}) does not match the directory name ({folder})"));
                        }
                    }
                }
            }
            match f.description.as_deref().map(str::trim) {
                None | Some("") => {
                    findings.push("frontmatter is missing a non-empty `description`".into())
                }
                Some(d) if d.len() < 16 => findings.push(
                    "frontmatter `description` is too short to be useful (< 16 chars)".into(),
                ),
                Some(_) => {}
            }
        }
        Err(s) => findings.push(format!("frontmatter is not valid YAML: {s}")),
    }
    if body.trim().is_empty() {
        findings.push("SKILL.md has no instruction body after the frontmatter".into());
    }
    findings
}

fn findings_text(f: Vec<Finding>) -> Vec<String> {
    f.into_iter().map(|f| f.message).collect()
}

// ---------------------------------------------------------------------------
// Schema-less views
// ---------------------------------------------------------------------------

fn json(v: &impl Serialize) -> String {
    serde_json::to_string(v).unwrap()
}

/// The `rewrite` values (the one schema-less walk of YAML input in the code:
/// `MockDecl::rewrite: Option<serde_json::Value>`), order-preserving JSON.
fn rewrites(decls: &[MockDecl]) -> Vec<String> {
    decls
        .iter()
        .filter_map(|d| d.rewrite.as_ref().map(json))
        .collect()
}

fn tree_to_json(y: &saphyr::YamlOwned) -> Value {
    use saphyr::{ScalarOwned, YamlOwned};
    match y {
        YamlOwned::Value(s) => match s {
            ScalarOwned::Null => Value::Null,
            ScalarOwned::Boolean(b) => Value::Bool(*b),
            ScalarOwned::Integer(i) => Value::from(*i),
            ScalarOwned::FloatingPoint(f) => serde_json::Number::from_f64(f.into_inner())
                .map(Value::Number)
                .unwrap_or_else(|| Value::String(format!("<float {}>", f.into_inner()))),
            ScalarOwned::String(s) => Value::String(s.clone()),
        },
        YamlOwned::Sequence(s) => Value::Array(s.iter().map(tree_to_json).collect()),
        YamlOwned::Mapping(m) => {
            let mut out = serde_json::Map::new();
            for (k, v) in m {
                let key = match tree_to_json(k) {
                    Value::String(s) => s,
                    other => other.to_string(),
                };
                out.insert(key, tree_to_json(v));
            }
            Value::Object(out)
        }
        YamlOwned::Tagged(tag, inner) => {
            let mut out = serde_json::Map::new();
            out.insert(format!("<tag {tag}>"), tree_to_json(inner));
            Value::Object(out)
        }
        YamlOwned::Representation(s, _, _) => Value::String(format!("<repr {s}>")),
        YamlOwned::Alias(n) => Value::String(format!("<alias {n}>")),
        YamlOwned::BadValue => Value::String("<bad value>".into()),
    }
}

fn saphyr_tree(text: &str) -> Result<String, String> {
    use saphyr::LoadableYamlNode;
    let docs = saphyr::YamlOwned::load_from_str(text).map_err(|e| e.to_string())?;
    match docs.as_slice() {
        [] => Ok("<no document>".into()),
        [one] => Ok(tree_to_json(one).to_string()),
        many => Err(format!("{} documents", many.len())),
    }
}

// ---------------------------------------------------------------------------
// Comparison and reporting
// ---------------------------------------------------------------------------

type Outcome = Result<String, String>;

fn dbg<T: Debug>(r: Result<T, String>) -> Outcome {
    r.map(|v| format!("{v:#?}"))
}

struct Diff {
    kind: Kind,
    origin: Origin,
    doc: String,
    check: String,
    before: Outcome,
    after: Outcome,
    /// Option presets under which serde-saphyr matches serde_yaml exactly.
    reproduced_by: Vec<&'static str>,
    /// Presets under which it accepts/rejects the same and reads the same
    /// value (error wording aside).
    accepts_like: Vec<&'static str>,
    /// serde-saphyr's outcome with `with_snippet: false`, when it differs.
    no_snippet: Option<Outcome>,
}

fn clip(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… [{} bytes]", &s[..end], s.len())
}

fn show(o: &Outcome) -> String {
    let (tag, body) = match o {
        Ok(s) => ("ok", s.as_str()),
        Err(s) => ("ERROR", s.as_str()),
    };
    format!("{tag}: {}", clip(body, 1200).replace('\n', "\n        "))
}

/// For two long successful outcomes, the region around their first
/// difference (so a one-byte change deep in a document is visible).
fn focus(a: &Outcome, b: &Outcome) -> Option<String> {
    let (Ok(a), Ok(b)) = (a, b) else { return None };
    if a.len() <= 1200 && b.len() <= 1200 {
        return None;
    }
    let at = a
        .char_indices()
        .zip(b.chars())
        .find(|((_, x), y)| x != y)
        .map_or(a.len().min(b.len()), |((i, _), _)| i);
    let mut start = at.saturating_sub(120);
    while !a.is_char_boundary(start) {
        start -= 1;
    }
    let window = |s: &str| {
        let mut end = (at + 120).min(s.len());
        while !s.is_char_boundary(end) {
            end += 1;
        }
        format!("{:?}", &s[start.min(s.len())..end])
    };
    Some(format!(
        "first difference at byte {at}:\n    serde_yaml   …{}…\n    serde-saphyr …{}…",
        window(a),
        window(b)
    ))
}

/// One check: run `f` with serde_yaml and serde-saphyr defaults; on a
/// difference, retry every preset. Error-vs-error differences in message text
/// are recorded too (no preset changes wording, so none is retried).
fn check(
    diffs: &mut Vec<Diff>,
    doc: &Doc,
    name: &str,
    f: &dyn Fn(&Engine) -> Outcome,
) -> bool {
    let before = f(&Engine::SerdeYaml);
    let after = f(&saphyr_default());
    if before == after {
        return true;
    }
    let mut o = Options::default();
    o.with_snippet = false;
    let no_snippet = Some(f(&Engine::Saphyr("with_snippet=false", o))).filter(|n| *n != after);
    let mut reproduced_by = Vec::new();
    let mut accepts_like = Vec::new();
    for p in presets() {
        let got = f(&p);
        if let Engine::Saphyr(n, _) = p {
            if got == before {
                reproduced_by.push(n);
            }
            let alike = match (&got, &before) {
                (Ok(x), Ok(y)) => x == y,
                (Err(_), Err(_)) => true,
                _ => false,
            };
            if alike {
                accepts_like.push(n);
            }
        }
    }
    diffs.push(Diff {
        kind: doc.kind,
        origin: doc.origin,
        doc: doc.label.clone(),
        check: name.into(),
        before,
        after,
        reproduced_by,
        accepts_like,
        no_snippet,
    });
    false
}

fn self_check(failures: &mut Vec<String>, doc: &Doc, what: &str, real: Outcome, replica: Outcome) {
    if real != replica {
        failures.push(format!(
            "{} {}: replica with serde_yaml != real loader\n  real:    {}\n  replica: {}",
            doc.label,
            what,
            show(&real),
            show(&replica)
        ));
    }
}

fn run_doc(doc: &Doc, diffs: &mut Vec<Diff>, failures: &mut Vec<String>) {
    let p = doc.path.clone();
    match doc.kind {
        Kind::Config => {
            self_check(
                failures,
                doc,
                "Config::load",
                dbg(Config::load(&p).map_err(|e| e.to_string())),
                dbg(config_load(&Engine::SerdeYaml, &p)),
            );
            check(diffs, doc, "typed read", &|e| dbg(config_load(e, &p)));
            check(diffs, doc, "rewrite values (schema-less)", &|e| {
                config_load(e, &p).map(|c| rewrites(&c.mocks).join("\n"))
                    .or_else(|_| Ok("<load failed: see typed read>".into()))
            });
        }
        Kind::Case => {
            self_check(
                failures,
                doc,
                "TestCase::load",
                dbg(TestCase::load(&p).map_err(|e| e.to_string())),
                dbg(case_load(&Engine::SerdeYaml, &p)),
            );
            check(diffs, doc, "typed read", &|e| dbg(case_load(e, &p)));
            check(diffs, doc, "rewrite values (schema-less)", &|e| {
                case_load(e, &p).map(|c| rewrites(&c.mocks).join("\n"))
                    .or_else(|_| Ok("<load failed: see typed read>".into()))
            });
        }
        Kind::Mocks => {
            check(diffs, doc, "typed read", &|e| dbg(mocks_load(e, &p)));
            check(diffs, doc, "rewrite values (schema-less)", &|e| {
                mocks_load(e, &p).map(|m| rewrites(&m).join("\n"))
                    .or_else(|_| Ok("<load failed: see typed read>".into()))
            });
        }
        Kind::Skill => {
            self_check(
                failures,
                doc,
                "load_skill",
                dbg(load_skill(&p).map_err(|e| e.to_string())),
                dbg(skill_load(&Engine::SerdeYaml, &p)),
            );
            self_check(
                failures,
                doc,
                "validate_skill",
                dbg(validate_skill(&p).map(findings_text).map_err(|e| e.to_string())),
                dbg(Ok(skill_validate(&Engine::SerdeYaml, &p))),
            );
            check(diffs, doc, "load_skill", &|e| dbg(skill_load(e, &p)));
            check(diffs, doc, "validate_skill findings", &|e| {
                dbg(Ok(skill_validate(e, &p)))
            });
        }
    }
    // Whole document into serde_json::Value (preserve_order), each crate.
    if let Some(text) = doc.yaml() {
        check(diffs, doc, "whole document -> serde_json::Value", &|e| {
            e.parse::<Value>(&text).map(|v| json(&v))
        });
    }
}

/// serde_yaml's Value vs saphyr's node tree, for every document.
fn tree_diffs(docs: &[Doc]) -> Vec<(String, Outcome, Outcome)> {
    let mut out = Vec::new();
    for doc in docs {
        let Some(text) = doc.yaml() else { continue };
        let before = serde_yaml::from_str::<Value>(&text)
            .map(|v| json(&v))
            .map_err(|e| e.to_string());
        let after = saphyr_tree(&text);
        if before != after {
            out.push((format!("{:?}/{:?} {}", doc.kind, doc.origin, doc.label), before, after));
        }
    }
    out
}

/// Classify every SKILL.md frontmatter difference (real and external) by cause.
fn frontmatter_causes(docs: &[Doc]) {
    let mut same = 0;
    let mut newline: Vec<(String, Vec<&str>)> = Vec::new();
    let mut other = Vec::new();
    let mut considered = 0;
    for doc in docs.iter().filter(|d| {
        d.kind == Kind::Skill && matches!(d.origin, Origin::Real | Origin::External)
    }) {
        let Some(fm) = doc.yaml() else { continue };
        considered += 1;
        let a = serde_yaml::from_str::<Frontmatter>(&fm).map_err(|e| e.to_string());
        let b = serde_saphyr::from_str::<Frontmatter>(&fm).map_err(|e| e.to_string());
        match (&a, &b) {
            (Ok(a), Ok(b)) => {
                let fields = [
                    ("name", &a.name, &b.name),
                    ("description", &a.description, &b.description),
                    ("license", &a.license, &b.license),
                ];
                let differing: Vec<_> = fields.iter().filter(|(_, x, y)| x != y).collect();
                if differing.is_empty() {
                    same += 1;
                } else if differing.iter().all(|(_, x, y)| {
                    matches!((x, y), (Some(x), Some(y)) if format!("{x}\n") == *y)
                }) {
                    newline.push((doc.label.clone(), differing.iter().map(|(n, _, _)| *n).collect()));
                } else {
                    other.push(format!("{}: {:?} vs {:?}", doc.label, a, b));
                }
            }
            (Err(_), Err(_)) => same += 1,
            _ => other.push(format!("{}: {:?} vs {:?}", doc.label, a.as_ref().map(|_| "ok"), b.as_ref().map(|_| "ok"))),
        }
    }
    println!("\n== SKILL.md frontmatter differences by cause (real + external, typed Frontmatter) ==");
    println!("{considered} frontmatters: {same} identical, {} differ only by a trailing \\n serde-saphyr adds to a field, {} other", newline.len(), other.len());
    let mut by_field = std::collections::BTreeMap::new();
    for (_, fields) in &newline {
        *by_field.entry(fields.join("+")).or_insert(0) += 1;
    }
    println!("  trailing-newline fields: {by_field:?}");
    for (label, _) in newline.iter().take(5) {
        println!("  e.g. {label}");
    }
    for o in &other {
        println!("  OTHER {}", clip(o, 600));
    }
}

// ---------------------------------------------------------------------------
// Writes
// ---------------------------------------------------------------------------

fn round_trip_config() -> Config {
    // `config_round_trips_through_yaml` in crates/skilltest-core/src/config.rs.
    Config {
        judge: Some(JudgeConfig::Api(ApiJudgeConfig {
            vendor: ApiVendor::Openai,
            api_key_env: Some("X".into()),
            base_url: None,
            timeout_secs: 30,
            curl_bin: "curl".into(),
            strict_json: false,
        })),
        ..Config::default()
    }
}

fn ser_grid() -> Vec<(String, SerializerOptions)> {
    let mut out = Vec::new();
    for compact in [true, false] {
        for braces in [true, false] {
            for y12 in [false, true] {
                for block in [true, false] {
                    for quote in [false, true] {
                        let mut o = SerializerOptions::default();
                        o.compact_list_indent = compact;
                        o.empty_as_braces = braces;
                        o.yaml_12 = y12;
                        o.prefer_block_scalars = block;
                        o.quote_all = quote;
                        out.push((
                            format!("compact_list_indent={compact} empty_as_braces={braces} yaml_12={y12} prefer_block_scalars={block} quote_all={quote}"),
                            o,
                        ));
                    }
                }
            }
        }
    }
    out
}

fn line_diff(a: &str, b: &str) -> usize {
    let (la, lb): (Vec<_>, Vec<_>) = (a.lines().collect(), b.lines().collect());
    let n = la.len().max(lb.len());
    (0..n).filter(|i| la.get(*i) != lb.get(*i)).count()
}

fn writes(name: &str, value: &Config) {
    println!("\n--- write: {name} ---");
    let before = serde_yaml::to_string(value).unwrap();
    let after = serde_saphyr::to_string(value).unwrap();
    println!("serde_yaml bytes ({}):\n{before}", before.len());
    if before == after {
        println!("serde-saphyr (default options): IDENTICAL");
    } else {
        println!("serde-saphyr default bytes ({}):\n{after}", after.len());
        println!("byte-level: {:?}\nvs          {:?}", before, after);
    }
    let mut matches = Vec::new();
    let mut best: Option<(usize, String, String)> = None;
    for (label, o) in ser_grid() {
        let out = serde_saphyr::to_string_with_options(value, o).unwrap();
        if out == before {
            matches.push(label.clone());
        }
        let d = line_diff(&before, &out);
        if best.as_ref().is_none_or(|(bd, _, _)| d < *bd) {
            best = Some((d, label, out));
        }
    }
    if matches.is_empty() {
        let (d, label, out) = best.unwrap();
        println!("no SerializerOptions combination (of {}) reproduces serde_yaml; closest ({d} differing lines): {label}\n{out}", ser_grid().len());
    } else {
        println!("SerializerOptions reproducing serde_yaml byte for byte: {matches:#?}");
    }
    // Cross-reads: does each crate read the other's output back to the value?
    let rt = |label: &str, r: Result<Config, String>| match r {
        Ok(c) if &c == value => println!("  {label}: reads back equal"),
        Ok(c) => println!("  {label}: reads back DIFFERENT: {c:?}"),
        Err(e) => println!("  {label}: ERROR {e}"),
    };
    rt("serde_yaml   <- serde-saphyr output", serde_yaml::from_str(&after).map_err(|e| e.to_string()));
    rt("serde-saphyr <- serde-saphyr output", serde_saphyr::from_str(&after).map_err(|e| e.to_string()));
    rt("serde-saphyr <- serde_yaml output", serde_saphyr::from_str(&before).map_err(|e| e.to_string()));
}

// ---------------------------------------------------------------------------
// Timing
// ---------------------------------------------------------------------------

/// Best (minimum) mean over 5 rounds of `iters` parses, in µs per parse: the
/// minimum is the least host-load-sensitive estimate on a shared machine.
fn time<F: FnMut()>(iters: u32, mut f: F) -> f64 {
    for _ in 0..iters / 10 {
        f();
    }
    (0..5)
        .map(|_| {
            let start = Instant::now();
            for _ in 0..iters {
                f();
            }
            start.elapsed().as_secs_f64() * 1e6 / f64::from(iters)
        })
        .fold(f64::INFINITY, f64::min)
}

fn timing(docs: &[Doc]) {
    let largest = docs
        .iter()
        .filter(|d| d.origin == Origin::Real && d.kind != Kind::Mocks)
        .filter_map(|d| d.yaml().map(|t| (t.len(), d, t)))
        .max_by_key(|(n, _, _)| *n)
        .unwrap();
    let (n, doc, text) = largest;
    println!("\n== parse time: largest real YAML document: {} ({n} bytes, {:?}) ==", doc.label, doc.kind);
    let iters = 5_000;
    macro_rules! typed {
        ($t:ty) => {{
            let a = time(iters, || {
                std::hint::black_box(serde_yaml::from_str::<$t>(&text).unwrap());
            });
            let b = time(iters, || {
                std::hint::black_box(serde_saphyr::from_str::<$t>(&text).unwrap());
            });
            (a, b)
        }};
    }
    let (a, b) = match doc.kind {
        Kind::Config => typed!(Config),
        Kind::Case => typed!(TestCase),
        _ => typed!(Value),
    };
    println!("typed ({:?}): serde_yaml {a:.2} µs/parse, serde-saphyr {b:.2} µs/parse (best of 5 rounds x {iters}, release build)", doc.kind);
    let (a, b) = typed!(Value);
    let c = time(iters, || {
        use saphyr::LoadableYamlNode;
        std::hint::black_box(saphyr::YamlOwned::load_from_str(&text).unwrap());
    });
    println!("schema-less: serde_yaml->Value {a:.2} µs, serde-saphyr->Value {b:.2} µs, saphyr YamlOwned tree {c:.2} µs");
}

// ---------------------------------------------------------------------------

fn main() {
    let root = repo_root();
    let scratch = std::env::temp_dir().join(format!("saphyr-diff-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();

    let mut docs = real_docs(&root);
    docs.extend(spike_docs());
    docs.extend(probe_docs(&scratch));
    let args: Vec<String> = std::env::args().skip(1).collect();
    for pair in args.chunks(2) {
        match pair {
            [flag, dir] if flag == "--external" => docs.extend(external_docs(Path::new(dir))),
            _ => {
                eprintln!("usage: run.sh [--external <checkout-dir>]...");
                std::process::exit(2);
            }
        }
    }

    println!("serde_yaml 0.9.34 vs serde-saphyr 1.3.0 (+ saphyr 0.2.1 tree)");
    for kind in [Kind::Config, Kind::Case, Kind::Skill, Kind::Mocks] {
        let count = |o| docs.iter().filter(|d| d.kind == kind && d.origin == o).count();
        println!(
            "  {:<42} real {:>2}, variant {:>2}, probe {:>2}, error-case {:>2}, external {:>3}",
            kind.label(),
            count(Origin::Real),
            count(Origin::Variant),
            count(Origin::Probe),
            count(Origin::Error),
            count(Origin::External)
        );
    }

    let mut diffs = Vec::new();
    let mut failures = Vec::new();
    for doc in &docs {
        run_doc(doc, &mut diffs, &mut failures);
    }

    for kind in [Kind::Config, Kind::Case, Kind::Skill, Kind::Mocks] {
        println!("\n== {} ==", kind.label());
        for origin in [
            Origin::Real,
            Origin::External,
            Origin::Variant,
            Origin::Probe,
            Origin::Error,
        ] {
            let n_docs = docs.iter().filter(|d| d.kind == kind && d.origin == origin).count();
            if n_docs == 0 {
                continue;
            }
            let these: Vec<_> = diffs
                .iter()
                .filter(|d| d.kind == kind && d.origin == origin)
                .collect();
            let n_diff_docs = {
                let mut v: Vec<_> = these.iter().map(|d| d.doc.as_str()).collect();
                v.dedup();
                v.len()
            };
            println!(
                "-- {origin:?}: {n_docs} documents, {n_diff_docs} with differences, {} differing checks",
                these.len()
            );
            for d in these {
                println!("[DIFF] {} :: {}", d.doc, d.check);
                println!("  serde_yaml   {}", show(&d.before));
                println!("  serde-saphyr {}", show(&d.after));
                if let Some(f) = focus(&d.before, &d.after) {
                    println!("  {f}");
                }
                if let Some(n) = &d.no_snippet {
                    println!("  serde-saphyr with_snippet=false {}", show(n));
                }
                let both_err = d.before.is_err() && d.after.is_err();
                if both_err {
                    println!("  (both reject; message text differs)");
                } else if d.reproduced_by.is_empty() {
                    println!("  reproduced by: no Options preset");
                } else {
                    println!("  reproduced by: {}", d.reproduced_by.join(", "));
                }
                if !both_err && d.reproduced_by.is_empty() {
                    if d.accepts_like.is_empty() {
                        println!("  same accept/reject + value (wording aside) under: no Options preset");
                    } else {
                        println!(
                            "  same accept/reject + value (wording aside) under: {}",
                            d.accepts_like.join(", ")
                        );
                    }
                }
            }
        }
    }

    frontmatter_causes(&docs);

    println!("\n== serde_yaml Value vs saphyr YamlOwned tree (schema-less, whole document) ==");
    let td = tree_diffs(&docs);
    println!("{} of {} documents differ", td.len(), docs.len());
    for (label, a, b) in td {
        println!("[DIFF] {label}\n  serde_yaml->Value {}\n  saphyr tree       {}", show(&a), show(&b));
    }

    println!("\n== writes ==");
    writes("config_round_trips_through_yaml's Config", &round_trip_config());
    writes("Config::default()", &Config::default());
    if let Ok(c) = Config::load(&root.join("examples/skilltest.yaml")) {
        writes("examples/skilltest.yaml as loaded", &c);
    }
    if let Ok(c) = config_load(&Engine::SerdeYaml, &spike_dir().join("fixtures/variants/config_command_api.yaml")) {
        writes("variants/config_command_api.yaml as loaded", &c);
    }

    timing(&docs);

    let _ = std::fs::remove_dir_all(&scratch);
    if !failures.is_empty() {
        eprintln!("\nREPLICA SELF-CHECK FAILURES:\n{}", failures.join("\n"));
        std::process::exit(1);
    }
    println!("\nreplica self-checks: all {} documents agree with skilltest-core's real loaders under serde_yaml", docs.len());
}
