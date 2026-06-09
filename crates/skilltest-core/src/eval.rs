//! Natural-language evaluations. An eval poses a criterion in plain English and
//! asks the provider's judge to score the transcript: a boolean assertion, or a
//! numeric score compared against a threshold.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// How a numeric score is compared to its threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Comparator {
    /// value >= threshold (the default).
    #[serde(alias = ">=")]
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

impl Default for Comparator {
    fn default() -> Self {
        Self::Gte
    }
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
}

impl Eval {
    /// The criterion text the judge sees.
    #[must_use]
    pub fn criterion(&self) -> &str {
        match self {
            Eval::Boolean { criterion, .. } | Eval::Numeric { criterion, .. } => criterion,
        }
    }

    /// A short label for reports: the explicit `name` if given, else the
    /// criterion.
    #[must_use]
    pub fn label(&self) -> &str {
        match self {
            Eval::Boolean {
                name, criterion, ..
            }
            | Eval::Numeric {
                name, criterion, ..
            } => name.as_deref().unwrap_or(criterion),
        }
    }

    /// Validate the eval's own parameters (independent of any transcript).
    ///
    /// # Errors
    /// [`Error::Invalid`] when a criterion is empty or a numeric scale is
    /// degenerate (`min >= max`) or the threshold falls outside `[min, max]`.
    pub fn validate(&self) -> Result<()> {
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
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum EvalDetail {
    Boolean {
        value: bool,
        expected: bool,
    },
    Numeric {
        value: f64,
        threshold: f64,
        comparator: Comparator,
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
        }
    }
}

/// The result of running one eval against a transcript.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
}
