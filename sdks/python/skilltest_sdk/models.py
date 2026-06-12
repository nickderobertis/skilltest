"""Typed views of the ``skilltest --format json`` contract, plus helpers.

The model classes live in ``_report.py`` / ``_validation.py``, which are
**generated** from the golden JSON Schemas in ``schemas/`` (themselves
generated from the CLI's Rust types). Never edit the generated modules by
hand — change the Rust types and run ``just gen-contract``; the gate fails
while anything is stale. This facade re-exports the generated models and adds
the hand-written conveniences, which the type checker keeps honest against the
generated fields.
"""

from __future__ import annotations

from ._report import (
    BooleanDetail,
    CaseRun,
    EvalOutcome,
    Message,
    NumericDetail,
    Report,
    Summary,
    Transcript,
    Usage,
)
from ._validation import ValidationFinding, ValidationReport

__all__ = [
    "BooleanDetail",
    "CaseRun",
    "EvalOutcome",
    "Message",
    "NumericDetail",
    "Report",
    "Summary",
    "Transcript",
    "Usage",
    "ValidationFinding",
    "ValidationReport",
    "assistant_text",
    "describe_failures",
    "failed_evals",
    "failed_runs",
]


def assistant_text(transcript: Transcript) -> str:
    """All assistant turns joined — handy for deterministic mix-in checks."""
    return "\n".join(m.content for m in transcript.messages if m.role == "assistant")


def failed_evals(run: CaseRun) -> list[EvalOutcome]:
    """The evals of a run that did not pass."""
    return [e for e in run.evals if not e.passed]


def failed_runs(report: Report) -> list[CaseRun]:
    """The runs of a report that did not pass."""
    return [r for r in report.runs if not r.passed]


def describe_failures(report: Report) -> str:
    """A one-line-per-failed-eval summary, for assertion messages."""
    lines: list[str] = []
    for run in failed_runs(report):
        for outcome in failed_evals(run):
            lines.append(
                f"{run.case} [{run.platform}/{run.model}] {outcome.label}: {outcome.reason}"
            )
    return "\n".join(lines)
