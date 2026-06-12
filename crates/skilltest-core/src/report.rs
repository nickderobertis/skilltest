//! Run results and the JSON report. The serialized shape here is the **stable
//! contract** the language SDKs parse. These types are the source of truth:
//! their JSON Schemas (via `skilltest schema`, goldens in `schemas/`) are what
//! the SDK contract tests compare their Pydantic/Zod models against.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::conversation::Transcript;
use crate::eval::EvalOutcome;
use crate::provider::Usage;
use crate::skill::Finding;

/// The result of running one test case on one (platform, model) pair.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CaseRun {
    /// The test case name.
    pub case: String,
    /// Absolute-ish path to the skill that was exercised.
    pub skill: String,
    /// The harness platform this run used.
    pub platform: String,
    /// The model this run used.
    pub model: String,
    /// True iff every eval in this run passed.
    pub passed: bool,
    /// Number of assistant turns produced.
    pub turns: usize,
    /// Per-eval outcomes, in declaration order.
    pub evals: Vec<EvalOutcome>,
    /// The full conversation, for debugging and deterministic mix-in checks.
    pub transcript: Transcript,
    /// Aggregated token/cost usage across every provider call in this run
    /// (skill turns + simulated-user turns + judge calls). Omitted when no
    /// usage was reported (e.g. the fake provider or a harness that doesn't
    /// surface usage).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

/// Aggregate pass/fail counts for a report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Summary {
    /// Distinct test cases represented.
    pub cases: usize,
    /// Total (case × platform × model) runs.
    pub runs: usize,
    /// Runs that passed.
    pub passed: usize,
    /// Runs that failed.
    pub failed: usize,
    /// Aggregated token/cost usage across every run in the report. Omitted
    /// when no run reported usage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

/// The top-level report for a `skilltest run` invocation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Report {
    /// True iff every run passed.
    pub passed: bool,
    /// Aggregate counts.
    pub summary: Summary,
    /// Every individual run.
    pub runs: Vec<CaseRun>,
}

impl Report {
    /// Build a report from runs, computing the summary and overall pass.
    #[must_use]
    pub fn new(runs: Vec<CaseRun>) -> Self {
        let mut case_names: Vec<&str> = runs.iter().map(|r| r.case.as_str()).collect();
        case_names.sort_unstable();
        case_names.dedup();
        let passed_runs = runs.iter().filter(|r| r.passed).count();
        let mut total_usage = Usage::default();
        for run in &runs {
            if let Some(u) = &run.usage {
                total_usage.add(u);
            }
        }
        let usage = (!total_usage.is_empty()).then_some(total_usage);
        let summary = Summary {
            cases: case_names.len(),
            runs: runs.len(),
            passed: passed_runs,
            failed: runs.len() - passed_runs,
            usage,
        };
        Report {
            passed: summary.failed == 0 && !runs.is_empty(),
            summary,
            runs,
        }
    }

    /// Serialize to pretty JSON (the `--format json` output).
    ///
    /// # Errors
    /// [`serde_json::Error`] only if a contained value cannot serialize, which
    /// should not happen for these types.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// A compact, human-readable summary line per run plus a total. Quiet by
    /// design: this is context the next reader has to parse.
    #[must_use]
    pub fn to_human(&self) -> String {
        let mut out = String::new();
        for run in &self.runs {
            let mark = if run.passed { "PASS" } else { "FAIL" };
            out.push_str(&format!(
                "{mark}  {} [{}/{}]\n",
                run.case, run.platform, run.model
            ));
            for eval in &run.evals {
                if !eval.passed {
                    out.push_str(&format!(
                        "      - {}: {} ({})\n",
                        eval.label,
                        eval.detail.summary(),
                        eval.reason
                    ));
                }
            }
        }
        out.push_str(&format!(
            "{}/{} runs passed\n",
            self.summary.passed, self.summary.runs
        ));
        if let Some(usage) = &self.summary.usage {
            let mut parts = Vec::new();
            if let Some(cost) = usage.cost_usd {
                parts.push(format!("${cost:.4}"));
            }
            if let (Some(i), Some(o)) = (usage.input_tokens, usage.output_tokens) {
                parts.push(format!("{} in / {} out tokens", i, o));
            } else {
                if let Some(i) = usage.input_tokens {
                    parts.push(format!("{i} input tokens"));
                }
                if let Some(o) = usage.output_tokens {
                    parts.push(format!("{o} output tokens"));
                }
            }
            if !parts.is_empty() {
                out.push_str(&format!("usage: {}\n", parts.join(", ")));
            }
        }
        out
    }
}

/// One problem found while validating a skill, as serialized in the
/// `skilltest validate --format json` output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ValidationFinding {
    /// The skill directory the finding is about.
    pub skill: String,
    /// What is wrong and how to fix it.
    pub message: String,
}

/// The top-level report for a `skilltest validate` invocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ValidationReport {
    /// True iff no findings were produced.
    pub valid: bool,
    /// Every finding, in discovery order.
    pub findings: Vec<ValidationFinding>,
}

impl ValidationReport {
    /// Build a validation report from raw findings.
    #[must_use]
    pub fn new(findings: &[Finding]) -> Self {
        ValidationReport {
            valid: findings.is_empty(),
            findings: findings
                .iter()
                .map(|f| ValidationFinding {
                    skill: f.skill.to_string_lossy().into_owned(),
                    message: f.message.clone(),
                })
                .collect(),
        }
    }

    /// Serialize to pretty JSON (the `--format json` output).
    ///
    /// # Errors
    /// [`serde_json::Error`] only if a contained value cannot serialize, which
    /// should not happen for these types.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}
