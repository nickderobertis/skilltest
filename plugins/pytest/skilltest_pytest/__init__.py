"""skilltest-pytest: run AI-skill tests and natural-language evals as pytest.

Public API:

    from skilltest_pytest import run_skill, validate_skill

    def test_greeter():
        report = run_skill("cases/greet.yaml")
        assert report.passed, report.describe_failures()
        # Mix in a deterministic check on the transcript:
        assert "Dr. Smith" in report.runs[0].transcript.assistant_text()

Or let the plugin collect ``*.skilltest.yaml`` files automatically.
"""

from __future__ import annotations

from .errors import SkilltestError, SkilltestProviderError, SkilltestUsageError
from .models import (
    BooleanDetail,
    CaseRun,
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
from .runner import run_skill, validate_skill

__all__ = [
    "BooleanDetail",
    "CaseRun",
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
