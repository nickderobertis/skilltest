"""Ordered stub responses (`stub(responses=...)`), driven through the real
binary + fake provider. The poller fixture skill checks `jobctl status
build-42` twice per turn, so a two-turn case makes four intercepted calls.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any

import pytest

from skilltest_sdk import (
    SkilltestUsageError,
    TestCase,
    boolean,
    called,
    describe_failures,
    run_skill,
    stub,
    user,
)
from skilltest_sdk.mock import ToolMock, compile_decls

RESPONSES: list[str | dict[str, str | int]] = [
    "build-42: queued",
    {"output": "build-42: failed (retrying)", "exit_code": 1},
    "build-42: passed",
]


def poller_case(fixtures: Path, status: ToolMock) -> TestCase:
    return TestCase(
        name="poll",
        skill=fixtures / "skills" / "poller",
        input="Watch build-42 until it settles.",
        user=user(
            "You are an operator waiting on a build.\nsay: Check again, please.", max_turns=2
        ),
        mocks=[status],
        evals=[
            called(status, times=4),
            boolean("the job reported `build-42: queued` then `build-42: failed (retrying)`"),
        ],
    )


def test_successive_calls_get_successive_responses(fixtures: Path) -> None:
    status = stub("jobctl status", tool="bash", responses=RESPONSES)
    report = run_skill(poller_case(fixtures, status))

    assert report.passed, describe_failures(report)
    # One recorded stub call per interception, each with its original input.
    status.assert_called_times(4)
    assert all(c.action == "stub" and c.command == "jobctl status build-42" for c in status.calls)
    # The model saw each response in turn, the count spanning both turns, and
    # the last response repeating.
    outputs = [
        (event.output or "").strip()
        for message in report.runs[0].transcript.messages
        if message.role == "assistant"
        for event in message.events or []
    ]
    assert outputs == [
        "build-42: queued",
        "build-42: failed (retrying)",
        "build-42: passed",
        "build-42: passed",
    ]


def test_responses_compile_to_the_yaml_sequence_form() -> None:
    (decl,) = compile_decls([stub("jobctl status", responses=RESPONSES)])
    assert decl["stub"] == [
        "build-42: queued",
        {"output": "build-42: failed (retrying)", "exit_code": 1},
        "build-42: passed",
    ]
    # An item without an exit code gets the default, like a single stub.
    (decl,) = compile_decls([stub("x", responses=[{"output": "a"}, "b"])])
    assert decl["stub"] == [{"output": "a", "exit_code": 0}, "b"]


@pytest.mark.parametrize(
    ("kwargs", "message"),
    [
        ({"responses": ["a"], "output": "b"}, "not both"),
        ({"responses": ["a"], "exit_code": 1}, "not both"),
        ({}, "needs `output`"),
        ({"responses": []}, "at least one response"),
        ({"responses": "ab"}, "needs a sequence"),
        ({"responses": ["a", ""]}, "response 1 must not be empty"),
        ({"responses": [{"output": "a", "exit_cod": 2}]}, "(?s)exit_cod.*Extra inputs"),
        ({"responses": [{"exit_code": 2}]}, "(?s)output.*Field required"),
        ({"responses": [{"output": "a", "exit_code": "2"}]}, "(?s)exit_code.*valid integer"),
        ({"responses": [3]}, "output string"),
    ],
)
# `Any`: these keyword combinations break stub()'s declared types on purpose, to
# reach the runtime checks an untyped caller would hit.
def test_malformed_stubs_are_refused_at_construction(kwargs: dict[str, Any], message: str) -> None:
    with pytest.raises(SkilltestUsageError, match=message):
        stub("jobctl status", **kwargs)


def test_single_output_stub_is_unchanged() -> None:
    (decl,) = compile_decls([stub("git push", output="ok")])
    assert decl["stub"] == {"output": "ok", "exit_code": 0}
    (decl,) = compile_decls([stub("git push", output="no", exit_code=2)])
    assert decl["stub"] == {"output": "no", "exit_code": 2}
