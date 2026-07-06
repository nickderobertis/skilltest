//! Run results and the JSON report. The serialized shape here is the **stable
//! contract** the language SDKs parse. These types are the source of truth:
//! their JSON Schemas (via `skilltest schema`, goldens in `schemas/`) are what
//! the SDK contract tests compare their Pydantic/Zod models against.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::conversation::Transcript;
use crate::eval::EvalOutcome;
use crate::mock::MockCall;
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
    /// Every tool call the mock/spy channel observed, in order, with the
    /// original (pre-rewrite) input and the verdict applied. `null` when the
    /// channel was off for this run (no `mocks`, no `spy`); an empty array
    /// means the channel was on and the skill made no tool calls — SDKs use
    /// that distinction so a spy on a channel-less run errs instead of reading
    /// as "zero calls".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mock_calls: Option<Vec<MockCall>>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation::Transcript;
    use crate::eval::{Comparator, EvalDetail, EvalOutcome};

    fn run(case: &str, passed: bool, evals: Vec<EvalOutcome>, usage: Option<Usage>) -> CaseRun {
        CaseRun {
            case: case.to_string(),
            skill: "/tmp/skill".to_string(),
            platform: "claude-code".to_string(),
            model: "sonnet".to_string(),
            passed,
            turns: 1,
            evals,
            transcript: Transcript::from_input("hi"),
            usage,
            mock_calls: None,
        }
    }

    fn bool_eval(label: &str, passed: bool) -> EvalOutcome {
        EvalOutcome {
            label: label.to_string(),
            passed,
            detail: EvalDetail::Boolean {
                value: passed,
                expected: true,
            },
            reason: "because".to_string(),
        }
    }

    #[test]
    fn new_computes_summary_and_dedups_cases() {
        let report = Report::new(vec![
            run("a", true, vec![bool_eval("x", true)], None),
            run("a", false, vec![bool_eval("y", false)], None),
            run("b", true, vec![bool_eval("z", true)], None),
        ]);
        // Two distinct cases, three runs, one failure -> overall fail.
        assert_eq!(report.summary.cases, 2);
        assert_eq!(report.summary.runs, 3);
        assert_eq!(report.summary.passed, 2);
        assert_eq!(report.summary.failed, 1);
        assert!(!report.passed);
        // No run reported usage, so the summary omits it.
        assert!(report.summary.usage.is_none());
    }

    #[test]
    fn empty_report_is_not_passed() {
        let report = Report::new(vec![]);
        assert!(!report.passed, "an empty run set is not a pass");
        assert_eq!(report.summary.runs, 0);
    }

    #[test]
    fn new_aggregates_usage_across_runs() {
        let report = Report::new(vec![
            run(
                "a",
                true,
                vec![bool_eval("x", true)],
                Some(Usage {
                    input_tokens: Some(10),
                    output_tokens: Some(2),
                    cost_usd: Some(0.01),
                }),
            ),
            run(
                "b",
                true,
                vec![bool_eval("y", true)],
                Some(Usage {
                    input_tokens: Some(5),
                    output_tokens: None,
                    cost_usd: Some(0.02),
                }),
            ),
        ]);
        let usage = report.summary.usage.unwrap();
        assert_eq!(usage.input_tokens, Some(15));
        assert_eq!(usage.output_tokens, Some(2));
        assert!((usage.cost_usd.unwrap() - 0.03).abs() < 1e-9);
    }

    #[test]
    fn to_json_round_trips() {
        let report = Report::new(vec![run("a", true, vec![bool_eval("x", true)], None)]);
        let json = report.to_json().unwrap();
        let parsed: Report = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, report);
    }

    #[test]
    fn to_human_lists_runs_and_failed_evals() {
        let numeric = EvalOutcome {
            label: "warmth".to_string(),
            passed: false,
            detail: EvalDetail::Numeric {
                value: 4.0,
                threshold: 7.0,
                comparator: Comparator::Gte,
            },
            reason: "too cold".to_string(),
        };
        let report = Report::new(vec![
            run("greets", true, vec![bool_eval("names", true)], None),
            run("warm", false, vec![numeric], None),
        ]);
        let human = report.to_human();
        assert!(human.contains("PASS  greets [claude-code/sonnet]"));
        assert!(human.contains("FAIL  warm"));
        // Only the failing eval is itemized, with its summary and reason.
        assert!(human.contains("warmth: 4 >= 7 (too cold)"), "got:\n{human}");
        assert!(human.contains("1/2 runs passed"));
    }

    #[test]
    fn to_human_renders_usage_line_variants() {
        // Cost + both token counts.
        let full = Report::new(vec![run(
            "a",
            true,
            vec![bool_eval("x", true)],
            Some(Usage {
                input_tokens: Some(100),
                output_tokens: Some(50),
                cost_usd: Some(0.1234),
            }),
        )]);
        let human = full.to_human();
        assert!(
            human.contains("usage: $0.1234, 100 in / 50 out tokens"),
            "got:\n{human}"
        );

        // Only an input-token count (no cost, no output) hits the singular branch.
        let partial = Report::new(vec![run(
            "a",
            true,
            vec![bool_eval("x", true)],
            Some(Usage {
                input_tokens: Some(7),
                output_tokens: None,
                cost_usd: None,
            }),
        )]);
        assert!(partial.to_human().contains("usage: 7 input tokens"));

        // Only an output-token count.
        let out_only = Report::new(vec![run(
            "a",
            true,
            vec![bool_eval("x", true)],
            Some(Usage {
                input_tokens: None,
                output_tokens: Some(9),
                cost_usd: None,
            }),
        )]);
        assert!(out_only.to_human().contains("usage: 9 output tokens"));
    }

    #[test]
    fn to_human_without_usage_has_no_usage_line() {
        let report = Report::new(vec![run("a", true, vec![bool_eval("x", true)], None)]);
        assert!(!report.to_human().contains("usage:"));
    }

    #[test]
    fn validation_report_new_and_json() {
        use crate::skill::Finding;
        let empty = ValidationReport::new(&[]);
        assert!(empty.valid);
        assert!(empty.findings.is_empty());

        let findings = vec![
            Finding {
                skill: std::path::PathBuf::from("/tmp/a"),
                message: "missing name".to_string(),
            },
            Finding {
                skill: std::path::PathBuf::from("/tmp/b"),
                message: "no body".to_string(),
            },
        ];
        let report = ValidationReport::new(&findings);
        assert!(!report.valid);
        assert_eq!(report.findings.len(), 2);
        assert_eq!(report.findings[0].skill, "/tmp/a");
        let json = report.to_json().unwrap();
        let parsed: ValidationReport = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, report);
    }
}
