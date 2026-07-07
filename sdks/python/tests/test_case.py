"""Code-defined test cases (`TestCase` + eval/user builders), driven through the
built binary + fake provider. A case written in Python must behave exactly like
its YAML twin — same conversation loop, evals, mocks, and exit codes."""

from __future__ import annotations

from pathlib import Path

import pytest

from skilltest_sdk import (
    SkilltestUsageError,
    TestCase,
    boolean,
    called,
    describe_failures,
    not_called,
    numeric,
    run_skill,
    spy,
    stream_skill,
    stub,
    user,
)


def skills(fixtures: Path) -> Path:
    return fixtures / "skills"


def test_inline_case_runs_like_a_yaml_case(fixtures: Path) -> None:
    case = TestCase(
        name="inline_greet",
        skill=skills(fixtures) / "greeter",
        input="Greet Dr. Smith, who has an appointment today.",
        evals=[
            boolean("the reply greets `Dr. Smith` by name"),
            boolean("the reply states the appointment is `confirmed`"),
        ],
    )
    report = run_skill(case)
    assert report.passed, describe_failures(report)
    assert report.summary.runs == 1
    assert report.runs[0].case == "inline_greet"


def test_inline_numeric_eval(fixtures: Path) -> None:
    case = TestCase(
        skill=skills(fixtures) / "greeter",
        input="Greet Dr. Smith",
        evals=[numeric("mentions `Dr. Smith`", min=0, max=10, threshold=5)],
    )
    report = run_skill(case)
    assert report.passed, describe_failures(report)
    # An unnamed case defaults to `case`.
    assert report.runs[0].case == "case"


def test_inline_failing_eval_is_reported_not_raised(fixtures: Path) -> None:
    case = TestCase(
        skill=skills(fixtures) / "greeter",
        input="Greet Dr. Smith",
        evals=[boolean("the reply mentions `a nonexistent phrase`")],
    )
    report = run_skill(case)
    assert not report.passed


def test_inline_multi_turn(fixtures: Path) -> None:
    case = TestCase(
        skill=skills(fixtures) / "greeter",
        input="I'd like to confirm my appointment, please.",
        user=user(
            "You are a terse patient confirming an appointment.\nsay: Yes, please go ahead.",
            done_when="the conversation has reached turns>=2",
            max_turns=4,
        ),
        evals=[boolean("the assistant confirmed the appointment (`confirmed`)")],
    )
    report = run_skill(case)
    assert report.passed, describe_failures(report)
    assert report.runs[0].turns == 2


def test_inline_case_with_named_mocks_and_call_evals(fixtures: Path) -> None:
    # A stub and a spy carried on the case itself: the case's `called`/
    # `not_called` evals reference them by name, and the same objects bind for
    # code-level assertions after the run.
    push = stub(pattern=r"git push( --force)?\b", output="Everything up-to-date", name="push")
    sudo = spy(contains="sudo", name="sudo")
    case = TestCase(
        skill=skills(fixtures) / "deployer",
        input="Deploy the app",
        mocks=[push, sudo],
        evals=[
            boolean("the reply reports `Everything up-to-date`"),
            called("push", times=1),
            not_called("sudo"),
        ],
    )
    report = run_skill(case)
    assert report.passed, describe_failures(report)
    # The case's mocks bind just like run-level `mocks=`.
    push.assert_called_once()
    assert push.calls[0].command == "git push origin main"
    sudo.assert_not_called()


def test_inline_case_run_level_mocks_still_compose(fixtures: Path) -> None:
    # A case with no mocks of its own can still take run-level `mocks=`.
    git = spy(tool="bash", pattern=r"\bgit\b")
    case = TestCase(
        skill=skills(fixtures) / "deployer",
        input="Deploy the app",
        evals=[boolean("the reply says `Deployment finished.`")],
    )
    report = run_skill(case, mocks=[git])
    assert report.passed, describe_failures(report)
    git.assert_called_times(2)


def test_inline_case_streams(fixtures: Path) -> None:
    case = TestCase(
        skill=skills(fixtures) / "tooluser",
        input="do the thing",
        evals=[boolean("ok")],
    )
    stream = stream_skill(case)
    names = []

    async def drain() -> None:
        async for ev in stream:
            names.append(ev.event.name)

    import asyncio

    asyncio.run(drain())
    assert stream.report is not None
    assert names  # the tooluser skill scripts at least one tool event


def test_malformed_inline_case_raises_usage_error(fixtures: Path) -> None:
    # No evals: the CLI validates the compiled case and rejects it loudly.
    case = TestCase(skill=skills(fixtures) / "greeter", input="hi", evals=[])
    with pytest.raises(SkilltestUsageError):
        run_skill(case)
