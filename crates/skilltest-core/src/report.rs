//! Run results and the JSON report. The serialized shape here is the **stable
//! contract** the pytest and vitest plugins parse, so changes must be made in
//! lockstep with the Pydantic models and the Zod schema.

use serde::{Deserialize, Serialize};

use crate::conversation::Transcript;
use crate::eval::EvalOutcome;

/// The result of running one test case on one (platform, model) pair.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
}

/// Aggregate pass/fail counts for a report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    /// Distinct test cases represented.
    pub cases: usize,
    /// Total (case × platform × model) runs.
    pub runs: usize,
    /// Runs that passed.
    pub passed: usize,
    /// Runs that failed.
    pub failed: usize,
}

/// The top-level report for a `skilltest run` invocation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
        let summary = Summary {
            cases: case_names.len(),
            runs: runs.len(),
            passed: passed_runs,
            failed: runs.len() - passed_runs,
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
        out
    }
}
