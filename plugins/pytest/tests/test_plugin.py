"""E2e tests for the pytest integration.

The happy path of auto-collection is also exercised by `collected/`
(`greet.skilltest.yaml` runs as part of this very suite); the `pytester` tests
here drive a *child* pytest end-to-end so the failure path — a collected case
whose eval fails — can be asserted on too.
"""

from __future__ import annotations

import asyncio
from pathlib import Path

import pytest

from skilltest_pytest import describe_failures, run_skill, stream_skill, tool_calls

SKILL_MD = """\
---
name: greeter
description: A local greeter skill used to exercise pytest auto-collection.
---
# Greeter

Greet the user by name.

<!-- fake-reply: Hello, Dr. Smith! Welcome to the clinic. -->
"""


def write_skill(root: Path) -> None:
    skill = root / "greeter"
    skill.mkdir()
    (skill / "SKILL.md").write_text(SKILL_MD)


def test_sdk_api_is_reexported_and_works(cases: Path) -> None:
    # One dependency is enough for a pytest suite: the SDK's code-level API is
    # available straight from skilltest_pytest.
    report = run_skill(cases / "greet_pass.yaml")
    assert report.passed, describe_failures(report)


def test_tool_events_and_streaming_reexported_and_work(cases: Path) -> None:
    # The tool-event and streaming surfaces are re-exported from the plugin too.
    report = run_skill(cases / "tool_events.yaml")
    assert [c.name for c in tool_calls(report.runs[0].transcript)] == ["edit_file", "bash"]

    async def go() -> list[str | None]:
        names: list[str | None] = []
        async for ev in stream_skill(cases / "tool_events.yaml"):
            names.append(ev.event.name)
        return names

    assert asyncio.run(go()) == ["edit_file", "bash"]


def test_collected_case_passes(pytester: pytest.Pytester) -> None:
    write_skill(pytester.path)
    pytester.makefile(
        ".skilltest.yaml",
        greet="""
        name: collected_greet
        skill: ./greeter
        input: "Greet Dr. Smith."
        evals:
          - type: boolean
            name: names-the-patient
            criterion: "the reply greets `Dr. Smith` by name"
        """,
    )
    result = pytester.runpytest_subprocess()
    result.assert_outcomes(passed=1)


def test_collected_case_failure_reports_judge_reason(pytester: pytest.Pytester) -> None:
    write_skill(pytester.path)
    pytester.makefile(
        ".skilltest.yaml",
        farewell="""
        name: collected_farewell
        skill: ./greeter
        input: "Greet Dr. Smith."
        evals:
          - type: boolean
            name: says-goodbye
            criterion: "the reply contains a `goodbye`"
        """,
    )
    result = pytester.runpytest_subprocess()
    result.assert_outcomes(failed=1)
    result.stdout.fnmatch_lines(["*skilltest case failed:*", "*says-goodbye*"])


def test_code_defined_case_is_reexported_and_runs(fixtures: Path) -> None:
    # The recommended form: build the whole case in code and hand it to
    # run_skill — the case API rides the same one-dependency re-export.
    from skilltest_pytest import TestCase, boolean, run_skill

    case = TestCase(
        skill=fixtures / "skills" / "greeter",
        input="Greet Dr. Smith, who has an appointment today.",
        evals=[boolean("the reply greets `Dr. Smith` by name")],
    )
    report = run_skill(case)
    assert report.passed, describe_failures(report)


def test_mock_api_is_reexported_and_binds(cases: Path) -> None:
    # The mock/spy API rides the one-dependency re-export; code-level mocks
    # intercept and bind through the plugin's SDK exactly as through the SDK.
    from skilltest_pytest import contains, matching, run_skill, spy, stub

    push = stub(pattern=r"git push( --force)?\b", output="Everything up-to-date")
    git = spy(tool="bash", pattern=r"\bgit\b")
    report = run_skill(cases / "deploy_plain.yaml", mocks=[push, git])
    assert report.passed
    push.assert_called_once()
    assert push.calls[0].command == "git push origin main"
    git.assert_called_with(command=contains("git status"))
    git.where(command=matching(r"\bsudo\b")).assert_not_called()
