//! `skilltest-core` — the library that powers the `skilltest` CLI and the
//! pytest/vitest plugins.
//!
//! The flow is: load a [`Config`] and one or more [`TestCase`]s, build a
//! [`Provider`] (the boundary to `oneharness` / a model), and hand both to a
//! [`Runner`], which drives each case into a conversation, scores the transcript
//! with natural-language [`Eval`]s, and returns a [`Report`]. The report's JSON
//! form is the stable contract the language plugins consume.
//!
//! Everything that crosses a trust boundary — config files, test-case YAML,
//! skill frontmatter, and every provider response — is parsed into a typed model
//! before use.

#![forbid(unsafe_code)]

pub mod config;
pub mod conversation;
pub mod error;
pub mod eval;
pub mod exit;
pub mod provider;
pub mod report;
pub mod runner;
pub mod skill;
pub mod testcase;

pub use config::{CommandConfig, Config, OneharnessConfig, Overrides, ProviderConfig};
pub use conversation::{Message, Role, Transcript};
pub use error::{Error, Result};
pub use eval::{Comparator, Eval, EvalDetail, EvalOutcome, JudgeValue};
pub use exit::ExitCode;
pub use provider::{
    supports_resume, AssistantTurn, CommandProvider, JudgeKind, JudgeQuery, JudgeVerdict,
    OneharnessProvider, Provider, SkillRef, Usage, UserTurn,
};
pub use report::{CaseRun, Report, Summary};
pub use runner::Runner;
pub use skill::{load_skill, validate_path, validate_skill, Finding, SkillDefinition};
pub use testcase::{discover_cases, SimulatedUser, TestCase};
