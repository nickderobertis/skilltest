//! Evaluations. The judge-backed kinds pose a criterion in plain English and
//! ask the provider's judge to score the transcript (a boolean assertion, or a
//! numeric score compared against a threshold). The deterministic kinds
//! (`called` / `not_called`) assert on the mock/spy channel's observed tool
//! calls — no judge, no flakiness — referencing a `mocks:` declaration by name.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::mock::{validate_where, FieldPredicate};

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

/// Assert a plain-English criterion holds (or, with `expected: false`, that
/// it does not).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BooleanEval {
    /// The criterion the judge evaluates against the transcript.
    pub criterion: String,
    /// What the judge's verdict must equal to pass. Defaults to `true`.
    #[serde(default = "default_true")]
    pub expected: bool,
    /// Optional human label for reports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Score a plain-English criterion on a numeric scale and compare it to a
/// threshold.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NumericEval {
    /// The criterion the judge scores.
    pub criterion: String,
    /// Inclusive lower bound of the scale.
    pub min: f64,
    /// Inclusive upper bound of the scale.
    pub max: f64,
    /// The passing threshold.
    pub threshold: f64,
    /// How the score is compared to `threshold`. Defaults to `>=`.
    #[serde(default)]
    pub comparator: Comparator,
    /// Optional human label for reports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Deterministic: assert the referenced mock/spy observed at least one
/// matching call (or exactly `times`). Evaluated against the mock channel's
/// records, not by a judge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CalledEval {
    /// The `mocks:` declaration (mock or spy) this asserts on, by name.
    pub mock: String,
    /// Exact required call count; absent means "at least once".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub times: Option<u64>,
    /// Optional per-field input predicates narrowing which calls count.
    #[serde(default, rename = "where", skip_serializing_if = "BTreeMap::is_empty")]
    pub r#where: BTreeMap<String, FieldPredicate>,
    /// Optional human label for reports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Deterministic: assert the referenced mock/spy observed **no** matching
/// call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NotCalledEval {
    /// The `mocks:` declaration (mock or spy) this asserts on, by name.
    pub mock: String,
    /// Optional per-field input predicates narrowing which calls count.
    #[serde(default, rename = "where", skip_serializing_if = "BTreeMap::is_empty")]
    pub r#where: BTreeMap<String, FieldPredicate>,
    /// Optional human label for reports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// An eval specification, as written in a test case's YAML (or compiled by an
/// SDK's case builders).
///
/// Newtype variants on purpose: serde cannot `deny_unknown_fields` on an
/// internally tagged enum, but it *does* enforce it on the variant structs, so
/// a typo'd eval field (`expcted:`) is a loud parse error instead of a
/// silently-applied default. The variant titles name the generated SDK model
/// for each union arm, so keep them stable: they are part of the SDK API
/// surface (the input contract, `schemas/case.schema.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Eval {
    #[schemars(title = "BooleanEval")]
    Boolean(BooleanEval),
    #[schemars(title = "NumericEval")]
    Numeric(NumericEval),
    #[schemars(title = "CalledEval")]
    Called(CalledEval),
    #[serde(rename = "not_called")]
    #[schemars(title = "NotCalledEval")]
    NotCalled(NotCalledEval),
}

impl Eval {
    /// The criterion text the judge sees; `None` for the deterministic
    /// (`called`/`not_called`) kinds, which no judge ever scores.
    #[must_use]
    pub fn criterion(&self) -> Option<&str> {
        match self {
            Eval::Boolean(BooleanEval { criterion, .. })
            | Eval::Numeric(NumericEval { criterion, .. }) => Some(criterion),
            Eval::Called(_) | Eval::NotCalled(_) => None,
        }
    }

    /// A short label for reports: the explicit `name` if given, else the
    /// criterion (judge kinds) or `called: <mock>` / `not_called: <mock>`.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Eval::Boolean(BooleanEval {
                name, criterion, ..
            })
            | Eval::Numeric(NumericEval {
                name, criterion, ..
            }) => name.as_deref().unwrap_or(criterion).to_string(),
            Eval::Called(CalledEval { name, mock, .. }) => {
                name.clone().unwrap_or_else(|| format!("called: {mock}"))
            }
            Eval::NotCalled(NotCalledEval { name, mock, .. }) => name
                .clone()
                .unwrap_or_else(|| format!("not_called: {mock}")),
        }
    }

    /// Validate the eval's own parameters (independent of any transcript).
    ///
    /// # Errors
    /// [`Error::Invalid`] when a criterion is empty, a numeric scale is
    /// degenerate (`min >= max`), the threshold falls outside `[min, max]`, a
    /// call eval references an empty mock name or asks for `times: 0`, or a
    /// `where` predicate is malformed.
    pub fn validate(&self) -> Result<()> {
        if let Some(criterion) = self.criterion() {
            if criterion.trim().is_empty() {
                return Err(Error::Invalid("an eval has an empty `criterion`".into()));
            }
        }
        match self {
            Eval::Numeric(NumericEval {
                min,
                max,
                threshold,
                ..
            }) => {
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
            Eval::Called(CalledEval {
                mock,
                times,
                r#where,
                ..
            }) => {
                if mock.trim().is_empty() {
                    return Err(Error::Invalid(
                        "a `called` eval has an empty `mock` reference".into(),
                    ));
                }
                if *times == Some(0) {
                    return Err(Error::Invalid(
                        "`called` with `times: 0` is ambiguous — use `type: not_called`".into(),
                    ));
                }
                validate_where(r#where, &format!("eval `{}`", self.label()))?;
            }
            Eval::NotCalled(NotCalledEval { mock, r#where, .. }) => {
                if mock.trim().is_empty() {
                    return Err(Error::Invalid(
                        "a `not_called` eval has an empty `mock` reference".into(),
                    ));
                }
                validate_where(r#where, &format!("eval `{}`", self.label()))?;
            }
            Eval::Boolean(_) => {}
        }
        Ok(())
    }

    /// Apply a deterministic call eval's pass rule to the number of matching
    /// observed calls, producing an outcome. Only meaningful for
    /// [`Eval::Called`]/[`Eval::NotCalled`]; the runner never routes judge
    /// kinds here.
    ///
    /// # Errors
    /// [`Error::Invalid`] if invoked on a judge-backed eval kind (a runner bug,
    /// surfaced loudly rather than scored vacuously).
    pub fn outcome_for_calls(&self, count: usize, observed: &str) -> Result<EvalOutcome> {
        let (times, negated, mock) = match self {
            Eval::Called(CalledEval { times, mock, .. }) => (*times, false, mock),
            Eval::NotCalled(NotCalledEval { mock, .. }) => (None, true, mock),
            _ => {
                return Err(Error::Invalid(
                    "outcome_for_calls invoked on a judge-backed eval".into(),
                ))
            }
        };
        let passed = if negated {
            count == 0
        } else {
            match times {
                Some(t) => count as u64 == t,
                None => count > 0,
            }
        };
        let expectation = if negated {
            "no matching calls".to_string()
        } else {
            match times {
                Some(t) => format!("exactly {t}"),
                None => "at least one".to_string(),
            }
        };
        let reason = if passed {
            format!("mock `{mock}` matched {count} call(s)")
        } else if observed.is_empty() {
            format!("mock `{mock}` matched {count} call(s), expected {expectation}; no tool calls were observed")
        } else {
            format!("mock `{mock}` matched {count} call(s), expected {expectation}; observed: {observed}")
        };
        Ok(EvalOutcome {
            label: self.label(),
            passed,
            detail: EvalDetail::Calls {
                count,
                times,
                negated,
            },
            reason,
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
            (Eval::Boolean(BooleanEval { expected, .. }), JudgeValue::Bool(value)) => {
                Ok(EvalOutcome {
                    label: self.label(),
                    passed: value == expected,
                    detail: EvalDetail::Boolean {
                        value: *value,
                        expected: *expected,
                    },
                    reason,
                })
            }
            (
                Eval::Numeric(NumericEval {
                    min,
                    max,
                    threshold,
                    comparator,
                    ..
                }),
                JudgeValue::Number(value),
            ) => {
                let clamped = value.clamp(*min, *max);
                Ok(EvalOutcome {
                    label: self.label(),
                    passed: comparator.satisfied(clamped, *threshold),
                    detail: EvalDetail::Numeric {
                        value: clamped,
                        threshold: *threshold,
                        comparator: *comparator,
                    },
                    reason,
                })
            }
            (Eval::Boolean(_), JudgeValue::Number(_)) => Err(Error::provider(
                "judge",
                "boolean eval received a numeric verdict",
            )),
            (Eval::Numeric(_), JudgeValue::Bool(_)) => Err(Error::provider(
                "judge",
                "numeric eval received a boolean verdict",
            )),
            // Deterministic kinds are scored via `outcome_for_calls`, never by
            // a judge verdict — routing one here is a runner bug.
            (Eval::Called(_) | Eval::NotCalled(_), _) => Err(Error::Invalid(
                "a call eval cannot be scored by a judge verdict".into(),
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
    /// The deterministic `called`/`not_called` verdict: how many observed calls
    /// matched, against what expectation.
    #[schemars(title = "CallsDetail")]
    Calls {
        /// Matching calls observed.
        count: usize,
        /// The exact count required (`times`); `null` means "at least one"
        /// (or, with `negated`, "none").
        #[serde(default, skip_serializing_if = "Option::is_none")]
        times: Option<u64>,
        /// True for `not_called` (the eval required absence).
        #[serde(default)]
        negated: bool,
    },
}

impl EvalDetail {
    /// A compact human description of the verdict, e.g. `8.0 >= 7`,
    /// `true (expected true)`, or `2 calls (expected exactly 1)`.
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
            EvalDetail::Calls {
                count,
                times,
                negated,
            } => {
                let noun = if *count == 1 { "call" } else { "calls" };
                let expected = if *negated {
                    "none".to_string()
                } else {
                    match times {
                        Some(t) => format!("exactly {t}"),
                        None => "at least 1".to_string(),
                    }
                };
                format!("{count} {noun} (expected {expected})")
            }
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

    #[test]
    fn numeric_threshold_gte_passes_at_boundary() {
        let eval = Eval::Numeric(NumericEval {
            criterion: "polite".into(),
            min: 0.0,
            max: 10.0,
            threshold: 7.0,
            comparator: Comparator::Gte,
            name: None,
        });
        let outcome = eval.outcome(&JudgeValue::Number(7.0), "ok".into()).unwrap();
        assert!(outcome.passed);
    }

    #[test]
    fn numeric_value_is_clamped_to_scale() {
        let eval = Eval::Numeric(NumericEval {
            criterion: "x".into(),
            min: 0.0,
            max: 10.0,
            threshold: 9.0,
            comparator: Comparator::Gte,
            name: None,
        });
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
        let eval = Eval::Boolean(BooleanEval {
            criterion: "leaks a secret".into(),
            expected: false,
            name: None,
        });
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
        let eval = Eval::Boolean(BooleanEval {
            criterion: "x".into(),
            expected: true,
            name: None,
        });
        assert!(eval
            .outcome(&JudgeValue::Number(1.0), String::new())
            .is_err());
    }

    #[test]
    fn degenerate_numeric_scale_is_invalid() {
        let eval = Eval::Numeric(NumericEval {
            criterion: "x".into(),
            min: 5.0,
            max: 5.0,
            threshold: 5.0,
            comparator: Comparator::Gte,
            name: None,
        });
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
        let bool_named = Eval::Boolean(BooleanEval {
            criterion: "is polite".into(),
            expected: true,
            name: Some("politeness".into()),
        });
        assert_eq!(bool_named.criterion(), Some("is polite"));
        assert_eq!(bool_named.label(), "politeness");

        let numeric_unnamed = Eval::Numeric(NumericEval {
            criterion: "warmth".into(),
            min: 0.0,
            max: 10.0,
            threshold: 5.0,
            comparator: Comparator::Gte,
            name: None,
        });
        assert_eq!(numeric_unnamed.criterion(), Some("warmth"));
        // Falls back to the criterion when unnamed.
        assert_eq!(numeric_unnamed.label(), "warmth");
    }

    #[test]
    fn validate_rejects_empty_criterion_and_out_of_range_threshold() {
        let empty = Eval::Boolean(BooleanEval {
            criterion: "   ".into(),
            expected: true,
            name: None,
        });
        assert!(empty.validate().is_err());

        let bad_threshold = Eval::Numeric(NumericEval {
            criterion: "x".into(),
            min: 0.0,
            max: 10.0,
            threshold: 11.0,
            comparator: Comparator::Gte,
            name: None,
        });
        assert!(bad_threshold.validate().is_err());

        // A well-formed numeric eval validates.
        let ok = Eval::Numeric(NumericEval {
            criterion: "x".into(),
            min: 0.0,
            max: 10.0,
            threshold: 7.0,
            comparator: Comparator::Gte,
            name: None,
        });
        ok.validate().unwrap();
    }

    #[test]
    fn outcome_rejects_numeric_eval_with_boolean_verdict() {
        let eval = Eval::Numeric(NumericEval {
            criterion: "x".into(),
            min: 0.0,
            max: 10.0,
            threshold: 5.0,
            comparator: Comparator::Gte,
            name: None,
        });
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

    #[test]
    fn called_and_not_called_parse_from_yaml() {
        let called: Eval = serde_yaml::from_str(
            "type: called\nmock: push\ntimes: 1\nwhere: { command: { contains: \"--force\" } }\n",
        )
        .unwrap();
        called.validate().unwrap();
        assert!(
            matches!(&called, Eval::Called(CalledEval { mock, times: Some(1), .. }) if mock == "push")
        );
        assert_eq!(called.label(), "called: push");
        assert!(called.criterion().is_none());

        let not_called: Eval = serde_yaml::from_str("type: not_called\nmock: danger\n").unwrap();
        not_called.validate().unwrap();
        assert_eq!(not_called.label(), "not_called: danger");

        let named: Eval =
            serde_yaml::from_str("type: called\nmock: push\nname: pushed once\n").unwrap();
        assert_eq!(named.label(), "pushed once");
    }

    #[test]
    fn call_eval_validation_is_loud() {
        let empty_mock: Eval = serde_yaml::from_str("type: called\nmock: \"\"\n").unwrap();
        assert!(empty_mock.validate().is_err());
        let zero_times: Eval = serde_yaml::from_str("type: called\nmock: m\ntimes: 0\n").unwrap();
        let err = zero_times.validate().unwrap_err();
        assert!(err.to_string().contains("not_called"), "{err}");
        let bad_where: Eval = serde_yaml::from_str(
            "type: not_called\nmock: m\nwhere: { command: { pattern: \"(\" } }\n",
        )
        .unwrap();
        assert!(bad_where.validate().is_err());
    }

    #[test]
    fn outcome_for_calls_covers_every_expectation() {
        let at_least_once: Eval = serde_yaml::from_str("type: called\nmock: push\n").unwrap();
        assert!(at_least_once.outcome_for_calls(2, "").unwrap().passed);
        let failed = at_least_once.outcome_for_calls(0, "bash(ls)").unwrap();
        assert!(!failed.passed);
        // The failure reason carries the observed calls, so the mismatch is
        // diagnosable from the assertion output alone.
        assert!(
            failed.reason.contains("observed: bash(ls)"),
            "{}",
            failed.reason
        );
        assert!(matches!(
            failed.detail,
            EvalDetail::Calls {
                count: 0,
                times: None,
                negated: false
            }
        ));

        let exactly: Eval = serde_yaml::from_str("type: called\nmock: push\ntimes: 2\n").unwrap();
        assert!(exactly.outcome_for_calls(2, "").unwrap().passed);
        assert!(!exactly.outcome_for_calls(1, "").unwrap().passed);

        let never: Eval = serde_yaml::from_str("type: not_called\nmock: danger\n").unwrap();
        assert!(never.outcome_for_calls(0, "").unwrap().passed);
        let violated = never.outcome_for_calls(1, "bash(rm -rf /)").unwrap();
        assert!(!violated.passed);
        assert!(matches!(
            violated.detail,
            EvalDetail::Calls { negated: true, .. }
        ));

        // Routing a judge eval here is a loud bug, not a vacuous score.
        let judge: Eval = serde_yaml::from_str("type: boolean\ncriterion: x\n").unwrap();
        assert!(judge.outcome_for_calls(0, "").is_err());
    }

    #[test]
    fn calls_detail_summary_reads_naturally() {
        let one = EvalDetail::Calls {
            count: 1,
            times: Some(1),
            negated: false,
        };
        assert_eq!(one.summary(), "1 call (expected exactly 1)");
        let none_wanted = EvalDetail::Calls {
            count: 2,
            times: None,
            negated: true,
        };
        assert_eq!(none_wanted.summary(), "2 calls (expected none)");
        let at_least = EvalDetail::Calls {
            count: 0,
            times: None,
            negated: false,
        };
        assert_eq!(at_least.summary(), "0 calls (expected at least 1)");
    }
}
