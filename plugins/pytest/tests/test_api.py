"""Code-API tests for the pytest plugin against the built binary + fake provider."""

from __future__ import annotations

from pathlib import Path

import pytest

from skilltest_pytest import (
    SkilltestProviderError,
    SkilltestUsageError,
    run_skill,
    validate_skill,
)


def test_happy_path_passes_and_exposes_transcript(cases: Path) -> None:
    report = run_skill(cases / "greet_pass.yaml")
    assert report.passed, report.describe_failures()
    assert report.summary.runs == 1
    # Deterministic mix-in check on top of the natural-language evals.
    assert "Dr. Smith" in report.runs[0].transcript.assistant_text()


def test_numeric_eval_detail_is_typed(cases: Path) -> None:
    report = run_skill(cases / "greet_numeric.yaml")
    assert report.passed
    detail = report.runs[0].evals[0].detail
    assert detail.kind == "numeric"
    assert detail.value >= detail.threshold


def test_failing_case_is_reported_not_raised(cases: Path) -> None:
    report = run_skill(cases / "greet_fail.yaml")
    assert not report.passed
    assert report.runs[0].failed_evals()
    assert "greet_fail" in report.describe_failures()


def test_multi_turn_runs_to_done_condition(cases: Path) -> None:
    report = run_skill(cases / "booking_multiturn.yaml")
    assert report.passed
    assert report.runs[0].turns == 2


def test_validate_accepts_good_skill(fixtures: Path) -> None:
    result = validate_skill(fixtures / "skills" / "greeter")
    assert result.valid
    assert result.findings == []


def test_validate_rejects_invalid_skill(fixtures: Path) -> None:
    result = validate_skill(fixtures / "skills" / "invalid")
    assert not result.valid
    assert any("description" in f.message for f in result.findings)


def test_missing_provider_raises_provider_error(cases: Path) -> None:
    with pytest.raises(SkilltestProviderError):
        run_skill(cases / "greet_pass.yaml", provider="/nonexistent/provider-bin")


def test_malformed_case_raises_usage_error(tmp_path: Path) -> None:
    bad = tmp_path / "bad.yaml"
    bad.write_text("skill: ./x\ninput: hi\nbogus: 1\nevals: []\n")
    with pytest.raises(SkilltestUsageError):
        run_skill(bad)
