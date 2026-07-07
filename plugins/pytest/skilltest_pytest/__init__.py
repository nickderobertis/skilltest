"""skilltest-pytest: run AI-skill tests and natural-language evals as pytest.

The pytest integration on top of [`skilltest-sdk`][skilltest_sdk], whose API is
re-exported here so a pytest suite needs only this one dependency. Define the
whole case in code (the recommended form):

    from skilltest_pytest import TestCase, run_skill, boolean, describe_failures

    def test_greeter():
        case = TestCase(
            skill="skills/greeter",
            input="Greet Dr. Smith.",
            evals=[boolean("the reply greets Dr. Smith by name")],
        )
        report = run_skill(case)
        assert report.passed, describe_failures(report)

Existing YAML cases stay first-class: ``run_skill("cases/greet.yaml")`` runs a
file, and any ``*.skilltest.yaml`` dropped next to your tests is auto-collected
as a test item with no code at all.
"""

from __future__ import annotations

from skilltest_sdk import (
    ENV_BIN,
    ENV_PROVIDER,
    BooleanDetail,
    CallsDetail,
    CaseRun,
    Eval,
    EvalOutcome,
    Matcher,
    Message,
    MockCall,
    NumericDetail,
    Report,
    SimulatedUser,
    SkillStream,
    SkilltestError,
    SkilltestProviderError,
    SkilltestUsageError,
    StreamEvent,
    Summary,
    TestCase,
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
    boolean,
    called,
    contains,
    deny,
    describe_failures,
    failed_evals,
    failed_runs,
    matching,
    not_called,
    numeric,
    rewrite,
    run_skill,
    spy,
    stream_skill,
    stub,
    tool_calls,
    user,
    validate_skill,
)

from .plugin import SkilltestFailure

__all__ = [
    "ENV_BIN",
    "ENV_PROVIDER",
    "BooleanDetail",
    "CallsDetail",
    "CaseRun",
    "Eval",
    "EvalOutcome",
    "Matcher",
    "Message",
    "MockCall",
    "NumericDetail",
    "Report",
    "SimulatedUser",
    "SkillStream",
    "SkilltestError",
    "SkilltestFailure",
    "SkilltestProviderError",
    "SkilltestUsageError",
    "StreamEvent",
    "Summary",
    "TestCase",
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
    "boolean",
    "called",
    "contains",
    "deny",
    "describe_failures",
    "failed_evals",
    "failed_runs",
    "matching",
    "not_called",
    "numeric",
    "rewrite",
    "run_skill",
    "spy",
    "stream_skill",
    "stub",
    "tool_calls",
    "user",
    "validate_skill",
]
