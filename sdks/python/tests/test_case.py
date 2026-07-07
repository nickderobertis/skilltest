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


def test_case_and_run_level_mocks_compose_with_run_level_winning(fixtures: Path) -> None:
    # Both channels at once: the case carries its own stub, and the run passes
    # another stub matching the same call. Run-level declarations are prepended
    # (first match wins), so the test-local rule shadows the case's own — and
    # both objects bind so the shadowing is assertable.
    case_push = stub(pattern=r"git push( --force)?\b", output="from-case", name="push")
    run_push = stub(pattern=r"git push( --force)?\b", output="from-run-level")
    case = TestCase(
        skill=skills(fixtures) / "deployer",
        input="Deploy the app",
        mocks=[case_push],
        evals=[boolean("the reply reports `from-run-level`")],
    )
    report = run_skill(case, mocks=[run_push])
    assert report.passed, describe_failures(report)
    run_push.assert_called_once()
    case_push.assert_not_called()


def test_case_spy_flag_turns_on_the_observation_channel(fixtures: Path) -> None:
    # `spy=True` alone (no mocks) must surface `mock_calls` on the report, so
    # "channel on with zero interceptions" is distinguishable from "channel
    # off" — and everything is recorded as allowed.
    case = TestCase(
        skill=skills(fixtures) / "deployer",
        input="Deploy the app",
        spy=True,
        evals=[boolean("the reply says `Deployment finished.`")],
    )
    report = run_skill(case)
    assert report.passed, describe_failures(report)
    records = report.runs[0].mock_calls
    assert records is not None and len(records) == 3
    assert all(r.action == "allow" for r in records)

    # Without the flag (and no mocks) the channel stays off.
    plain = TestCase(
        skill=skills(fixtures) / "deployer",
        input="Deploy the app",
        evals=[boolean("the reply says `Deployment finished.`")],
    )
    assert run_skill(plain).runs[0].mock_calls is None


def test_streaming_binds_case_level_mocks_on_completion(fixtures: Path) -> None:
    push = stub(pattern=r"git push( --force)?\b", output="Everything up-to-date", name="push")
    case = TestCase(
        skill=skills(fixtures) / "deployer",
        input="Deploy the app",
        mocks=[push],
        evals=[boolean("the reply reports `Everything up-to-date`")],
    )
    stream = stream_skill(case)

    async def drain() -> None:
        async for _ in stream:
            pass

    import asyncio

    asyncio.run(drain())
    assert stream.report is not None and stream.report.passed
    push.assert_called_once()
    assert push.calls[0].command == "git push origin main"


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


def test_builders_compile_to_the_kitchen_sink_golden(fixtures: Path) -> None:
    """The input-contract pin: a maximal case built with every builder feature
    must compile to exactly `tests/fixtures/contract/case_kitchen_sink.json`.
    The same golden is pinned Rust-side (construction + strict parse + an
    executable e2e run) and by the TypeScript builders, so drift in any
    direction breaks a named test. If the contract gains a field: update the
    golden, the Rust construction, and every SDK's builders together."""
    import json

    from skilltest_sdk import contains, deny, rewrite

    push = stub(
        tool="bash",
        pattern=r"git push( --force)?\b",
        where={"command": contains("origin")},
        output="Everything up-to-date",
        name="push",
    )
    danger = deny(contains="rm -rf", message="destructive commands are blocked", name="danger")
    status = rewrite(
        where={"command": "git status"}, input={"command": "git status --short"}, name="status"
    )
    sudo = spy(tool="bash", contains="sudo", name="sudo")
    case = TestCase(
        name="kitchen_sink",
        skill="tests/fixtures/skills/deployer",
        input="Deploy the app",
        user=user(
            "a terse operator\nsay: Yes, proceed.",
            done_when="the conversation has reached turns>=1",
            max_turns=3,
        ),
        mocks=[push, danger, status, sudo],
        evals=[
            boolean("the reply mentions `flying pigs`", expected=False, name="no-nonsense"),
            numeric(
                "mentions `Deployment finished.`",
                min=0,
                max=10,
                threshold=5,
                comparator=">",
                name="finished",
            ),
            called("push", times=1, where={"command": contains("origin")}, name="pushed-once"),
            not_called("sudo", name="no-sudo"),
        ],
    )

    golden_path = fixtures / "contract" / "case_kitchen_sink.json"
    golden = json.loads(golden_path.read_text())
    assert case._compile() == golden, (
        "the compiled case drifted from the kitchen-sink golden — "
        "update the golden, the Rust construction, and both SDKs' builders together"
    )
