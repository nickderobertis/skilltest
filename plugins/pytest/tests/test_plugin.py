"""E2e tests for the pytest integration.

The happy path of auto-collection is also exercised by `collected/`
(`greet.skilltest.yaml` runs as part of this very suite); the `pytester` tests
here drive a *child* pytest end-to-end so the failure path — a collected case
whose eval fails — can be asserted on too.
"""

from __future__ import annotations

from pathlib import Path

import pytest

from skilltest_pytest import describe_failures, run_skill

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
