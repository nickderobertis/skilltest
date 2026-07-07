"""skilltest-sdk: the Python SDK for the ``skilltest`` CLI.

A thin, typed wrapper around the CLI and nothing else: run test cases, validate
skills, and get back models mirroring the ``--format json`` contract. The
models are generated from the CLI's own JSON Schemas (``just gen-contract``),
so they cannot drift from the binary. Test frameworks build on this —
``skilltest-pytest`` adds pytest collection on top.

Define the whole case in code (the recommended form):

    from skilltest_sdk import TestCase, run_skill, boolean, describe_failures

    case = TestCase(
        skill="skills/greeter",
        input="Greet Dr. Smith, who has an appointment today.",
        evals=[boolean("the reply greets Dr. Smith by name")],
    )
    report = run_skill(case)
    assert report.passed, describe_failures(report)

``run_skill`` also takes a path to an existing test-case YAML file (or a
directory of them), and the pytest plugin auto-discovers ``*.skilltest.yaml``.
"""

from __future__ import annotations

from .case import (
    Eval,
    MockRefEval,
    SimulatedUser,
    TestCase,
    boolean,
    called,
    not_called,
    numeric,
    user,
)
from .errors import (
    ProviderErrorKind,
    SkilltestAuthError,
    SkilltestError,
    SkilltestModelNotFoundError,
    SkilltestOverloadedError,
    SkilltestProtocolError,
    SkilltestProviderError,
    SkilltestQuotaError,
    SkilltestRateLimitError,
    SkilltestSpawnError,
    SkilltestTimeoutError,
    SkilltestUsageError,
)
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
    ReportError,
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
    "Eval",
    "EvalOutcome",
    "Matcher",
    "Message",
    "MockCall",
    "MockRefEval",
    "NumericDetail",
    "ProviderErrorKind",
    "Report",
    "ReportError",
    "SimulatedUser",
    "SkillStream",
    "SkilltestAuthError",
    "SkilltestError",
    "SkilltestModelNotFoundError",
    "SkilltestOverloadedError",
    "SkilltestProtocolError",
    "SkilltestProviderError",
    "SkilltestQuotaError",
    "SkilltestRateLimitError",
    "SkilltestSpawnError",
    "SkilltestTimeoutError",
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
