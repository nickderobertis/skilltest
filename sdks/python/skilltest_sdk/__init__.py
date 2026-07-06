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
from .mock import (
    Matcher,
    ToolCall,
    ToolMock,
    ToolSpy,
    anything,
    contains,
    deny,
    matching,
    rewrite,
    spy,
    stub,
)
from .models import (
    BooleanDetail,
    CallsDetail,
    CaseRun,
    EvalOutcome,
    Message,
    MockCall,
    NumericDetail,
    Report,
    Summary,
    ToolEvent,
    Transcript,
    Usage,
    ValidationFinding,
    ValidationReport,
    assistant_text,
    describe_failures,
    failed_evals,
    failed_runs,
    tool_calls,
)
from .runner import ENV_BIN, ENV_PROVIDER, run_skill, validate_skill
from .stream import SkillStream, StreamEvent, stream_skill

__all__ = [
    "ENV_BIN",
    "ENV_PROVIDER",
    "BooleanDetail",
    "CallsDetail",
    "CaseRun",
    "EvalOutcome",
    "Matcher",
    "Message",
    "MockCall",
    "NumericDetail",
    "Report",
    "SkillStream",
    "SkilltestError",
    "SkilltestProviderError",
    "SkilltestUsageError",
    "StreamEvent",
    "Summary",
    "ToolCall",
    "ToolEvent",
    "ToolMock",
    "ToolSpy",
    "Transcript",
    "Usage",
    "ValidationFinding",
    "ValidationReport",
    "anything",
    "assistant_text",
    "contains",
    "deny",
    "describe_failures",
    "failed_evals",
    "failed_runs",
    "matching",
    "rewrite",
    "run_skill",
    "spy",
    "stream_skill",
    "stub",
    "tool_calls",
    "validate_skill",
]
