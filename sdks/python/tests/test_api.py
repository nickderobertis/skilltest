"""Code-API e2e tests for the SDK against the built binary + fake provider."""

from __future__ import annotations

from pathlib import Path
from typing import Any, get_args

import pytest

from skilltest_sdk import (
    NumericDetail,
    ProviderErrorKind,
    SkilltestProviderError,
    SkilltestUsageError,
    assistant_text,
    describe_failures,
    failed_evals,
    run_skill,
    tool_calls,
    validate_skill,
)


def test_happy_path_passes_and_exposes_transcript(cases: Path) -> None:
    report = run_skill(cases / "greet_pass.yaml")
    assert report.passed, describe_failures(report)
    assert report.summary.runs == 1
    # Deterministic mix-in check on top of the natural-language evals.
    assert "Dr. Smith" in assistant_text(report.runs[0].transcript)


def test_tool_calls_are_exposed_for_analysis(cases: Path) -> None:
    report = run_skill(cases / "tool_events.yaml")
    calls = tool_calls(report.runs[0].transcript)
    assert [c.name for c in calls] == ["edit_file", "bash"]
    assert calls[1].input == {"command": 'git commit -m "update config"'}


def test_numeric_eval_detail_is_typed(cases: Path) -> None:
    report = run_skill(cases / "greet_numeric.yaml")
    assert report.passed
    detail = report.runs[0].evals[0].detail
    assert isinstance(detail, NumericDetail)
    assert detail.value >= detail.threshold


def test_failing_case_is_reported_not_raised(cases: Path) -> None:
    report = run_skill(cases / "greet_fail.yaml")
    assert not report.passed
    assert failed_evals(report.runs[0])
    assert "greet_fail" in describe_failures(report)


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


def test_missing_binary_raises_provider_error(cases: Path) -> None:
    with pytest.raises(SkilltestProviderError):
        run_skill(cases / "greet_pass.yaml", bin="/nonexistent/skilltest-bin")


def _fake_oneharness_config(tmp_path: Path, results_json: str) -> Path:
    """A config pointing the oneharness provider at a scripted fake that emits
    ``results_json`` — so a classified failure can be exercised offline."""
    oh = tmp_path / "oneharness"
    oh.write_text(f"#!/bin/sh\ncat >/dev/null\nprintf '%s' '{results_json}'\n")
    oh.chmod(0o755)
    cfg = tmp_path / "skilltest.yaml"
    cfg.write_text(
        "provider:\n"
        "  kind: oneharness\n"
        f"  bin: {oh}\n"
        "  judge_harness: claude-code\n"
        "  timeout_secs: 5\n"
        "platforms: [claude-code]\n"
        "models: [sonnet]\n"
    )
    return cfg


def test_provider_error_carries_structured_kind(
    cases: Path, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    # oneharness reports a deadline as `status: "timeout"`; the SDK surfaces it as
    # a typed `kind`, so consumers branch on the category instead of the message.
    monkeypatch.delenv("SKILLTEST_PROVIDER", raising=False)
    cfg = _fake_oneharness_config(
        tmp_path, '{"results":[{"status":"timeout","stderr":"deadline exceeded"}]}'
    )
    with pytest.raises(SkilltestProviderError) as exc:
        run_skill(cases / "greet_pass.yaml", config=cfg)
    assert exc.value.kind == "timeout"
    assert exc.value.context == "oneharness:claude-code"


def test_classified_auth_failure_carries_its_kind(
    cases: Path, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.delenv("SKILLTEST_PROVIDER", raising=False)
    cfg = _fake_oneharness_config(
        tmp_path,
        '{"results":[{"status":"error","failure_kind":"auth","error":"no creds"}]}',
    )
    with pytest.raises(SkilltestProviderError) as exc:
        run_skill(cases / "greet_pass.yaml", config=cfg)
    assert exc.value.kind == "auth"


def test_usage_error_has_no_kind(tmp_path: Path) -> None:
    bad = tmp_path / "bad.yaml"
    bad.write_text("skill: ./x\ninput: hi\nbogus: 1\nevals: []\n")
    with pytest.raises(SkilltestUsageError) as exc:
        run_skill(bad)
    # A usage error is not a provider failure — it carries no classification.
    assert not hasattr(exc.value, "kind")


def _literal_strings(annotation: Any) -> set[str]:
    """Every string literal reachable in a typing annotation (recursing through
    ``Optional``/unions into the ``Literal`` members)."""
    found: set[str] = set()
    for arg in get_args(annotation):
        if isinstance(arg, str):
            found.add(arg)
        else:
            found |= _literal_strings(arg)
    return found


def test_provider_error_kind_matches_generated_contract() -> None:
    # Drift guard: the hand-written `ProviderErrorKind` literal must match the
    # generated `ReportError.kind` vocabulary (the contract gate regenerates the
    # latter from the Rust enum, but not this alias).
    from skilltest_sdk._error import ReportError

    generated = _literal_strings(ReportError.model_fields["kind"].annotation)
    assert generated == set(get_args(ProviderErrorKind))
