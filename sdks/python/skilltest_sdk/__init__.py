"""skilltest-sdk: the Python SDK for the ``skilltest`` CLI.

A thin, typed wrapper around the CLI and nothing else: run test cases, validate
skills, and get back models mirroring the ``--format json`` contract. The
models are generated from the CLI's own JSON Schemas (``just gen-contract``),
so they cannot drift from the binary. Test frameworks build on this —
``skilltest-pytest`` adds pytest collection on top.

    from skilltest_sdk import run_skill, describe_failures, assistant_text

    report = run_skill("cases/greet.yaml")
    assert report.passed, describe_failures(report)
    assert "Dr. Smith" in assistant_text(report.runs[0].transcript)
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
    assistant_text,
    describe_failures,
    failed_evals,
    failed_runs,
)
from .runner import ENV_BIN, ENV_PROVIDER, run_skill, validate_skill

__all__ = [
    "ENV_BIN",
    "ENV_PROVIDER",
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
    "assistant_text",
    "describe_failures",
    "failed_evals",
    "failed_runs",
    "run_skill",
    "validate_skill",
]
