"""skilltest-sdk: the Python SDK for the ``skilltest`` CLI.

A thin, typed wrapper around the CLI and nothing else: run test cases, validate
skills, and get back models mirroring the ``--format json`` contract. Test
frameworks build on this — ``skilltest-pytest`` adds pytest collection on top.

    from skilltest_sdk import run_skill, validate_skill

    report = run_skill("cases/greet.yaml")
    assert report.passed, report.describe_failures()
    assert "Dr. Smith" in report.runs[0].transcript.assistant_text()
"""

from __future__ import annotations

from .errors import SkilltestError, SkilltestProviderError, SkilltestUsageError
from .models import (
    BooleanDetail,
    CaseRun,
    Comparator,
    EvalOutcome,
    Message,
    NumericDetail,
    Report,
    Summary,
    Transcript,
    Usage,
    ValidationFinding,
    ValidationReport,
)
from .runner import ENV_BIN, ENV_PROVIDER, run_skill, validate_skill

__all__ = [
    "ENV_BIN",
    "ENV_PROVIDER",
    "BooleanDetail",
    "CaseRun",
    "Comparator",
    "EvalOutcome",
    "Message",
    "NumericDetail",
    "Report",
    "SkilltestError",
    "SkilltestProviderError",
    "SkilltestUsageError",
    "Summary",
    "Transcript",
    "Usage",
    "ValidationFinding",
    "ValidationReport",
    "run_skill",
    "validate_skill",
]
