"""skilltest-pytest: run AI-skill tests and natural-language evals as pytest.

The pytest integration on top of [`skilltest-sdk`][skilltest_sdk]: drop a
``*.skilltest.yaml`` next to your other tests and pytest collects it as a test
item. The SDK's code-level API is re-exported here for convenience, so a pytest
suite only needs one dependency:

    from skilltest_pytest import run_skill, validate_skill

    def test_greeter():
        report = run_skill("cases/greet.yaml")
        assert report.passed, describe_failures(report)
        # Mix in a deterministic check on the transcript:
        assert "Dr. Smith" in assistant_text(report.runs[0].transcript)
"""

from __future__ import annotations

from skilltest_sdk import (
    ENV_BIN,
    ENV_PROVIDER,
    BooleanDetail,
    CallsDetail,
    CaseRun,
    EvalOutcome,
    Matcher,
    Message,
    MockCall,
    NumericDetail,
    Report,
    SkillStream,
    SkilltestError,
    SkilltestProviderError,
    SkilltestUsageError,
    StreamEvent,
    Summary,
    ToolCall,
    ToolEvent,
    ToolMock,
    ToolSpy,
    Transcript,
    Usage,
    ValidationFinding,
    ValidationReport,
    anything,
    assistant_text,
    contains,
    deny,
    describe_failures,
    failed_evals,
    failed_runs,
    matching,
    rewrite,
    run_skill,
    spy,
    stream_skill,
    stub,
    tool_calls,
    validate_skill,
)

from .plugin import SkilltestFailure

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
    "SkilltestFailure",
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
