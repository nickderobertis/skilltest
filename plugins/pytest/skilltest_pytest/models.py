"""Pydantic models mirroring the ``skilltest --format json`` contract.

Every field crossing the process boundary from the CLI is parsed through these
models before any test code touches it, so a contract drift surfaces as a clear
validation error rather than a ``KeyError`` deep in a test.
"""

from __future__ import annotations

from typing import Annotated, Literal

from pydantic import BaseModel, ConfigDict, Field


class _Model(BaseModel):
    # Ignore unknown keys so a newer CLI that adds fields stays readable by an
    # older plugin; required fields are still enforced.
    model_config = ConfigDict(extra="ignore")


class Message(_Model):
    role: Literal["user", "assistant", "system"]
    content: str


class Transcript(_Model):
    messages: list[Message]

    def assistant_text(self) -> str:
        """All assistant turns joined — handy for deterministic mix-in checks."""
        return "\n".join(m.content for m in self.messages if m.role == "assistant")


class Usage(_Model):
    """Token / cost usage aggregated for a run or the whole report.

    Each field is independently optional because not every harness reports every
    signal (cost is commonly absent on subscription auth).
    """

    input_tokens: int | None = None
    output_tokens: int | None = None
    cost_usd: float | None = None


class BooleanDetail(_Model):
    kind: Literal["boolean"]
    value: bool
    expected: bool


class NumericDetail(_Model):
    kind: Literal["numeric"]
    value: float
    threshold: float
    comparator: str


EvalDetail = Annotated[BooleanDetail | NumericDetail, Field(discriminator="kind")]


class EvalOutcome(_Model):
    label: str
    passed: bool
    detail: EvalDetail
    reason: str


class CaseRun(_Model):
    case: str
    skill: str
    platform: str
    model: str
    passed: bool
    turns: int
    evals: list[EvalOutcome]
    transcript: Transcript
    usage: Usage | None = None

    def failed_evals(self) -> list[EvalOutcome]:
        return [e for e in self.evals if not e.passed]


class Summary(_Model):
    cases: int
    runs: int
    passed: int
    failed: int
    usage: Usage | None = None


class Report(_Model):
    passed: bool
    summary: Summary
    runs: list[CaseRun]

    def failed_runs(self) -> list[CaseRun]:
        return [r for r in self.runs if not r.passed]

    def describe_failures(self) -> str:
        """A one-line-per-failed-eval summary, for assertion messages."""
        lines: list[str] = []
        for run in self.failed_runs():
            for outcome in run.failed_evals():
                lines.append(
                    f"{run.case} [{run.platform}/{run.model}] {outcome.label}: {outcome.reason}"
                )
        return "\n".join(lines)


class ValidationFinding(_Model):
    skill: str
    message: str


class ValidationReport(_Model):
    valid: bool
    findings: list[ValidationFinding]
