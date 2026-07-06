//! Natural-language evaluations. An eval poses a criterion in plain English and
//! asks the provider's judge to score the transcript: a boolean assertion, or a
//! numeric score compared against a threshold.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// How a numeric score is compared to its threshold.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Comparator {
    /// value >= threshold (the default).
    #[serde(alias = ">=")]
    #[default]
    Gte,
    /// value > threshold.
    #[serde(alias = ">")]
    Gt,
    /// value <= threshold.
    #[serde(alias = "<=")]
    Lte,
    /// value < threshold.
    #[serde(alias = "<")]
    Lt,
}

impl Comparator {
    fn satisfied(self, value: f64, threshold: f64) -> bool {
        match self {
            Comparator::Gte => value >= threshold,
            Comparator::Gt => value > threshold,
            Comparator::Lte => value <= threshold,
            Comparator::Lt => value < threshold,
        }
    }

    fn symbol(self) -> &'static str {
        match self {
            Comparator::Gte => ">=",
            Comparator::Gt => ">",
            Comparator::Lte => "<=",
            Comparator::Lt => "<",
        }
    }
}

/// The default boolean expectation (the criterion should hold).
fn default_true() -> bool {
    true
}

/// An eval specification, as written in a test case's YAML.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Eval {
    /// Assert a plain-English criterion holds (or, with `expected: false`, that
    /// it does not).
    Boolean {
        /// The criterion the judge evaluates against the transcript.
        criterion: String,
        /// What the judge's verdict must equal to pass. Defaults to `true`.
        #[serde(default = "default_true")]
        expected: bool,
        /// Optional human label for reports.
        #[serde(default)]
        name: Option<String>,
    },
    /// Score a plain-English criterion on a numeric scale and compare it to a
    /// threshold.
    Numeric {
        /// The criterion the judge scores.
        criterion: String,
        /// Inclusive lower bound of the scale.
        min: f64,
        /// Inclusive upper bound of the scale.
        max: f64,
        /// The passing threshold.
        threshold: f64,
        /// How the score is compared to `threshold`. Defaults to `>=`.
        #[serde(default)]
        comparator: Comparator,
        /// Optional human label for reports.
        #[serde(default)]
        name: Option<String>,
    },
    /// A deterministic, **judge-free** assertion over the skill's normalized tool
    /// events (from oneharness `--events`) — behavioral correctness, not text.
    /// Counts the `tool_call` events matching the optional `tool` name and
    /// `input_contains` substring, then checks that count against `min`/`max`.
    /// Expresses the issue's cases: "ran git commit" (`input_contains: "git
    /// commit"`, `min: 1`), "never ran rm -rf" (`input_contains: "rm -rf"`,
    /// `max: 0`), "at most 3 tool calls" (`max: 3`), "edited config.yaml"
    /// (`input_contains: "config.yaml"`, `min: 1`).
    Tool {
        /// Count only `tool_call` events whose normalized name equals this
        /// (case-insensitive). Omit to count every tool call.
        #[serde(default)]
        tool: Option<String>,
        /// Count only events whose rendered input JSON contains this substring.
        /// Omit to not filter on input.
        #[serde(default)]
        input_contains: Option<String>,
        /// Inclusive minimum number of matching calls required to pass.
        #[serde(default)]
        min: Option<usize>,
        /// Inclusive maximum number of matching calls allowed to pass.
        #[serde(default)]
        max: Option<usize>,
        /// Optional human label for reports.
        #[serde(default)]
        name: Option<String>,
    },
}

impl Eval {
    /// The criterion text the judge sees. Empty for a behavioral [`Eval::Tool`],
    /// which is scored deterministically and never reaches the judge.
    #[must_use]
    pub fn criterion(&self) -> &str {
        match self {
            Eval::Boolean { criterion, .. } | Eval::Numeric { criterion, .. } => criterion,
            Eval::Tool { .. } => "",
        }
    }

    /// A short label for reports: the explicit `name` if given, else the
    /// criterion (or a generic label for a behavioral tool eval).
    #[must_use]
    pub fn label(&self) -> &str {
        match self {
            Eval::Boolean {
                name, criterion, ..
            }
            | Eval::Numeric {
                name, criterion, ..
            } => name.as_deref().unwrap_or(criterion),
            Eval::Tool { name, .. } => name.as_deref().unwrap_or("tool-call assertion"),
        }
    }

    /// Whether this eval is scored deterministically from the transcript's tool
    /// events ([`Eval::Tool`]) rather than by the natural-language judge.
    #[must_use]
    pub fn is_behavioral(&self) -> bool {
        matches!(self, Eval::Tool { .. })
    }

    /// Validate the eval's own parameters (independent of any transcript).
    ///
    /// # Errors
    /// [`Error::Invalid`] when a criterion is empty or a numeric scale is
    /// degenerate (`min >= max`) or the threshold falls outside `[min, max]`, or
    /// a `tool` eval bounds nothing / has `min > max`.
    pub fn validate(&self) -> Result<()> {
        if let Eval::Tool { min, max, .. } = self {
            if min.is_none() && max.is_none() {
                return Err(Error::Invalid(
                    "a `tool` eval needs at least one of `min`/`max` to assert against".into(),
                ));
            }
            if let (Some(mn), Some(mx)) = (min, max) {
                if mn > mx {
                    return Err(Error::Invalid(format!(
                        "`tool` eval bound is inverted: min ({mn}) must be <= max ({mx})"
                    )));
                }
            }
            return Ok(());
        }
        if self.criterion().trim().is_empty() {
            return Err(Error::Invalid("an eval has an empty `criterion`".into()));
        }
        if let Eval::Numeric {
            min,
            max,
            threshold,
            ..
        } = self
        {
            if min >= max {
                return Err(Error::Invalid(format!(
                    "numeric eval scale is degenerate: min ({min}) must be < max ({max})"
                )));
            }
            if threshold < min || threshold > max {
                return Err(Error::Invalid(format!(
                    "numeric eval threshold ({threshold}) is outside the scale [{min}, {max}]"
                )));
            }
        }
        Ok(())
    }

    /// Score a behavioral [`Eval::Tool`] deterministically against a transcript's
    /// normalized tool events — no judge call. Counts the `tool_call` events
    /// matching the `tool` name and `input_contains` filters, then checks the
    /// count against `min`/`max`.
    ///
    /// # Errors
    /// [`Error::Invalid`] if called on a non-behavioral eval.
    pub fn evaluate_over(
        &self,
        transcript: &crate::conversation::Transcript,
    ) -> Result<EvalOutcome> {
        let Eval::Tool {
            tool,
            input_contains,
            min,
            max,
            ..
        } = self
        else {
            return Err(Error::Invalid(
                "evaluate_over is only valid for a behavioral `tool` eval".into(),
            ));
        };
        let count = transcript
            .messages
            .iter()
            .flat_map(|m| &m.events)
            .filter(|e| e.kind == "tool_call")
            .filter(|e| {
                tool.as_ref()
                    .is_none_or(|t| e.name.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(t)))
            })
            .filter(|e| {
                input_contains.as_ref().is_none_or(|sub| {
                    e.input
                        .as_ref()
                        .is_some_and(|v| v.to_string().contains(sub))
                })
            })
            .count();
        let passed = min.is_none_or(|m| count >= m) && max.is_none_or(|m| count <= m);
        let bounds = match (min, max) {
            (Some(mn), Some(mx)) => format!("expected between {mn} and {mx}"),
            (Some(mn), None) => format!("expected at least {mn}"),
            (None, Some(mx)) => format!("expected at most {mx}"),
            (None, None) => "no bound".to_string(),
        };
        Ok(EvalOutcome {
            label: self.label().to_string(),
            passed,
            detail: EvalDetail::Tool {
                count,
                min: *min,
                max: *max,
            },
            reason: format!("matched {count} tool call(s); {bounds}"),
        })
    }

    /// Apply this eval's pass rule to a raw judge value, producing an outcome.
    ///
    /// `raw` is the value the judge returned: `JudgeValue::Bool` for boolean
    /// evals, `JudgeValue::Number` for numeric. A mismatch is a provider error.
    ///
    /// # Errors
    /// [`Error::Provider`] if the judge returned the wrong value kind for this
    /// eval.
    pub fn outcome(&self, raw: &JudgeValue, reason: String) -> Result<EvalOutcome> {
        match (self, raw) {
            (Eval::Boolean { expected, .. }, JudgeValue::Bool(value)) => Ok(EvalOutcome {
                label: self.label().to_string(),
                passed: value == expected,
                detail: EvalDetail::Boolean {
                    value: *value,
                    expected: *expected,
                },
                reason,
            }),
            (
                Eval::Numeric {
                    min,
                    max,
                    threshold,
                    comparator,
                    ..
                },
                JudgeValue::Number(value),
            ) => {
                let clamped = value.clamp(*min, *max);
                Ok(EvalOutcome {
                    label: self.label().to_string(),
                    passed: comparator.satisfied(clamped, *threshold),
                    detail: EvalDetail::Numeric {
                        value: clamped,
                        threshold: *threshold,
                        comparator: *comparator,
                    },
                    reason,
                })
            }
            (Eval::Boolean { .. }, JudgeValue::Number(_)) => Err(Error::provider(
                "judge",
                "boolean eval received a numeric verdict",
            )),
            (Eval::Numeric { .. }, JudgeValue::Bool(_)) => Err(Error::provider(
                "judge",
                "numeric eval received a boolean verdict",
            )),
            // A behavioral `tool` eval is scored deterministically via
            // `evaluate_over`, never by the judge — reaching here is a bug.
            (Eval::Tool { .. }, _) => Err(Error::provider(
                "judge",
                "a behavioral `tool` eval must be scored via evaluate_over, not the judge",
            )),
        }
    }
}

/// The raw value a judge returns: either a boolean or a number, matching the
/// eval kind. Deserialized untagged from the provider's `value` field.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JudgeValue {
    Bool(bool),
    Number(f64),
}

/// The kind-specific detail of an eval outcome, for reporting.
///
/// The variant titles name the generated SDK model for each union arm, so keep
/// them stable: they are part of the SDK API surface.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum EvalDetail {
    #[schemars(title = "BooleanDetail")]
    Boolean { value: bool, expected: bool },
    #[schemars(title = "NumericDetail")]
    Numeric {
        value: f64,
        threshold: f64,
        comparator: Comparator,
    },
    /// A behavioral tool-event assertion: how many matching `tool_call` events
    /// were counted, and the `min`/`max` bounds they were checked against.
    #[schemars(title = "ToolDetail")]
    Tool {
        count: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min: Option<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max: Option<usize>,
    },
}

impl EvalDetail {
    /// A compact human description of the verdict, e.g. `8.0 >= 7` or
    /// `true (expected true)`.
    #[must_use]
    pub fn summary(&self) -> String {
        match self {
            EvalDetail::Boolean { value, expected } => {
                format!("{value} (expected {expected})")
            }
            EvalDetail::Numeric {
                value,
                threshold,
                comparator,
            } => format!("{value} {} {threshold}", comparator.symbol()),
            EvalDetail::Tool { count, min, max } => match (min, max) {
                (Some(mn), Some(mx)) => format!("{count} tool call(s) (want {mn}..={mx})"),
                (Some(mn), None) => format!("{count} tool call(s) (want >= {mn})"),
                (None, Some(mx)) => format!("{count} tool call(s) (want <= {mx})"),
                (None, None) => format!("{count} tool call(s)"),
            },
        }
    }
}

/// The result of running one eval against a transcript.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EvalOutcome {
    /// The eval's label (name or criterion).
    pub label: String,
    /// Whether the eval passed.
    pub passed: bool,
    /// Kind-specific verdict detail.
    pub detail: EvalDetail,
    /// The judge's stated reason.
    pub reason: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation::{Message, ToolEvent, Transcript};

    /// A transcript with one assistant turn carrying the given tool_call events.
    fn transcript_with(calls: &[(&str, &str)]) -> Transcript {
        let events = calls
            .iter()
            .enumerate()
            .map(|(i, (name, cmd))| ToolEvent {
                kind: "tool_call".into(),
                name: Some((*name).into()),
                input: Some(serde_json::json!({ "command": cmd })),
                output: None,
                index: i,
            })
            .collect();
        Transcript {
            messages: vec![
                Message::user("do it"),
                Message::assistant("done").with_events(events),
            ],
        }
    }

    #[test]
    fn tool_eval_counts_matching_calls_and_checks_bounds() {
        let t = transcript_with(&[("bash", "git commit -m x"), ("bash", "ls"), ("edit", "y")]);
        // "ran git commit at least once" — matches by input substring.
        let ran_commit = Eval::Tool {
            tool: None,
            input_contains: Some("git commit".into()),
            min: Some(1),
            max: None,
            name: Some("ran git commit".into()),
        };
        let o = ran_commit.evaluate_over(&t).unwrap();
        assert!(o.passed, "{}", o.reason);
        assert_eq!(o.label, "ran git commit");
        assert!(matches!(o.detail, EvalDetail::Tool { count: 1, .. }));

        // "never ran rm -rf" — max 0 matches passes when absent.
        let no_rm = Eval::Tool {
            tool: None,
            input_contains: Some("rm -rf".into()),
            min: None,
            max: Some(0),
            name: None,
        };
        assert!(no_rm.evaluate_over(&t).unwrap().passed);

        // "at most 2 tool calls" — 3 calls fails.
        let at_most_two = Eval::Tool {
            tool: None,
            input_contains: None,
            min: None,
            max: Some(2),
            name: None,
        };
        let o = at_most_two.evaluate_over(&t).unwrap();
        assert!(!o.passed);
        assert!(matches!(o.detail, EvalDetail::Tool { count: 3, .. }));
    }

    #[test]
    fn tool_eval_filters_by_tool_name_case_insensitively() {
        let t = transcript_with(&[("Bash", "a"), ("edit", "b"), ("bash", "c")]);
        let two_bash = Eval::Tool {
            tool: Some("bash".into()),
            input_contains: None,
            min: Some(2),
            max: Some(2),
            name: None,
        };
        assert!(two_bash.evaluate_over(&t).unwrap().passed);
    }

    #[test]
    fn tool_eval_validates_bounds() {
        // No bound is a usage error.
        assert!(Eval::Tool {
            tool: None,
            input_contains: None,
            min: None,
            max: None,
            name: None,
        }
        .validate()
        .is_err());
        // Inverted bound is a usage error.
        assert!(Eval::Tool {
            tool: None,
            input_contains: None,
            min: Some(3),
            max: Some(1),
            name: None,
        }
        .validate()
        .is_err());
        // A single valid bound is fine, and the eval is behavioral.
        let ok = Eval::Tool {
            tool: None,
            input_contains: None,
            min: Some(1),
            max: None,
            name: None,
        };
        assert!(ok.validate().is_ok());
        assert!(ok.is_behavioral());
        assert_eq!(ok.criterion(), "");
    }

    #[test]
    fn numeric_threshold_gte_passes_at_boundary() {
        let eval = Eval::Numeric {
            criterion: "polite".into(),
            min: 0.0,
            max: 10.0,
            threshold: 7.0,
            comparator: Comparator::Gte,
            name: None,
        };
        let outcome = eval.outcome(&JudgeValue::Number(7.0), "ok".into()).unwrap();
        assert!(outcome.passed);
    }

    #[test]
    fn numeric_value_is_clamped_to_scale() {
        let eval = Eval::Numeric {
            criterion: "x".into(),
            min: 0.0,
            max: 10.0,
            threshold: 9.0,
            comparator: Comparator::Gte,
            name: None,
        };
        // Judge over-reports 12 -> clamped to 10, still passes.
        let outcome = eval
            .outcome(&JudgeValue::Number(12.0), String::new())
            .unwrap();
        assert!(outcome.passed);
        assert!(matches!(
            outcome.detail,
            EvalDetail::Numeric { value, .. } if (value - 10.0).abs() < f64::EPSILON
        ));
    }

    #[test]
    fn boolean_expected_false_inverts() {
        let eval = Eval::Boolean {
            criterion: "leaks a secret".into(),
            expected: false,
            name: None,
        };
        let pass = eval
            .outcome(&JudgeValue::Bool(false), String::new())
            .unwrap();
        assert!(pass.passed);
        let fail = eval
            .outcome(&JudgeValue::Bool(true), String::new())
            .unwrap();
        assert!(!fail.passed);
    }

    #[test]
    fn kind_mismatch_is_provider_error() {
        let eval = Eval::Boolean {
            criterion: "x".into(),
            expected: true,
            name: None,
        };
        assert!(eval
            .outcome(&JudgeValue::Number(1.0), String::new())
            .is_err());
    }

    #[test]
    fn degenerate_numeric_scale_is_invalid() {
        let eval = Eval::Numeric {
            criterion: "x".into(),
            min: 5.0,
            max: 5.0,
            threshold: 5.0,
            comparator: Comparator::Gte,
            name: None,
        };
        assert!(eval.validate().is_err());
    }

    #[test]
    fn comparator_parses_from_symbol() {
        let c: Comparator = serde_yaml::from_str("\">=\"").unwrap();
        assert_eq!(c, Comparator::Gte);
        let c: Comparator = serde_yaml::from_str("lt").unwrap();
        assert_eq!(c, Comparator::Lt);
    }

    #[test]
    fn every_comparator_satisfied_and_symbol() {
        assert!(Comparator::Gte.satisfied(5.0, 5.0));
        assert!(Comparator::Gt.satisfied(6.0, 5.0));
        assert!(!Comparator::Gt.satisfied(5.0, 5.0));
        assert!(Comparator::Lte.satisfied(5.0, 5.0));
        assert!(Comparator::Lt.satisfied(4.0, 5.0));
        assert!(!Comparator::Lt.satisfied(5.0, 5.0));
        assert_eq!(Comparator::Gte.symbol(), ">=");
        assert_eq!(Comparator::Gt.symbol(), ">");
        assert_eq!(Comparator::Lte.symbol(), "<=");
        assert_eq!(Comparator::Lt.symbol(), "<");
    }

    #[test]
    fn criterion_and_label_for_both_kinds() {
        let bool_named = Eval::Boolean {
            criterion: "is polite".into(),
            expected: true,
            name: Some("politeness".into()),
        };
        assert_eq!(bool_named.criterion(), "is polite");
        assert_eq!(bool_named.label(), "politeness");

        let numeric_unnamed = Eval::Numeric {
            criterion: "warmth".into(),
            min: 0.0,
            max: 10.0,
            threshold: 5.0,
            comparator: Comparator::Gte,
            name: None,
        };
        assert_eq!(numeric_unnamed.criterion(), "warmth");
        // Falls back to the criterion when unnamed.
        assert_eq!(numeric_unnamed.label(), "warmth");
    }

    #[test]
    fn validate_rejects_empty_criterion_and_out_of_range_threshold() {
        let empty = Eval::Boolean {
            criterion: "   ".into(),
            expected: true,
            name: None,
        };
        assert!(empty.validate().is_err());

        let bad_threshold = Eval::Numeric {
            criterion: "x".into(),
            min: 0.0,
            max: 10.0,
            threshold: 11.0,
            comparator: Comparator::Gte,
            name: None,
        };
        assert!(bad_threshold.validate().is_err());

        // A well-formed numeric eval validates.
        let ok = Eval::Numeric {
            criterion: "x".into(),
            min: 0.0,
            max: 10.0,
            threshold: 7.0,
            comparator: Comparator::Gte,
            name: None,
        };
        ok.validate().unwrap();
    }

    #[test]
    fn outcome_rejects_numeric_eval_with_boolean_verdict() {
        let eval = Eval::Numeric {
            criterion: "x".into(),
            min: 0.0,
            max: 10.0,
            threshold: 5.0,
            comparator: Comparator::Gte,
            name: None,
        };
        assert!(eval
            .outcome(&JudgeValue::Bool(true), String::new())
            .is_err());
    }

    #[test]
    fn eval_detail_summary_for_both_kinds() {
        let boolean = EvalDetail::Boolean {
            value: true,
            expected: false,
        };
        assert_eq!(boolean.summary(), "true (expected false)");
        let numeric = EvalDetail::Numeric {
            value: 8.0,
            threshold: 7.0,
            comparator: Comparator::Gte,
        };
        assert_eq!(numeric.summary(), "8 >= 7");
    }

    #[test]
    fn judge_value_deserializes_untagged() {
        let b: JudgeValue = serde_json::from_str("true").unwrap();
        assert!(matches!(b, JudgeValue::Bool(true)));
        let n: JudgeValue = serde_json::from_str("3.5").unwrap();
        assert!(matches!(n, JudgeValue::Number(v) if (v - 3.5).abs() < 1e-9));
    }
}
