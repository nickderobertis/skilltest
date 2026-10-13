"""SDK mock/spy objects through the REAL oneharness binary (the hermetic
middle tier, see docs/e2e.md): the fake-claude shim stands in for the harness
and executes the ephemerally installed mock hook, so `run_skill(mocks=[...])`
is proven against the real seam — rules compile, `--mocks` delivery, the real
`oneharness mock` responder, the spy JSONL, and binding.

oneharness now ships as the SDK's `oneharness-cli` dependency, so under a synced
venv these run against the real binary. Still skipped (not failed) when
`oneharness` is not on PATH, e.g. a bare checkout without `uv sync`.
"""

from __future__ import annotations

import shutil
from pathlib import Path

import pytest

from skilltest_sdk import contains, run_skill, spy, stub

FIXTURES = Path(__file__).resolve().parents[3] / "tests" / "fixtures"

pytestmark = pytest.mark.skipif(
    shutil.which("oneharness") is None,
    reason="needs oneharness on PATH (just install-oneharness)",
)

OH_FIXTURES = FIXTURES / "oneharness"
CASE = OH_FIXTURES / "cases" / "spy_plain.yaml"


@pytest.fixture
def oneharness_env(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> Path:
    """Point the run at real oneharness + the shim: drop the fake-provider env
    default (so the config's oneharness provider applies) and route the
    claude-code harness to the scripted shim. Returns a config file path."""
    monkeypatch.delenv("SKILLTEST_PROVIDER", raising=False)
    monkeypatch.setenv("ONEHARNESS_BIN_CLAUDE_CODE", str(OH_FIXTURES / "fake-claude.sh"))
    config = tmp_path / "skilltest.yaml"
    config.write_text(
        "provider:\n"
        "  kind: oneharness\n"
        "  bin: oneharness\n"
        "  judge_harness: claude-code\n"
        "  timeout_secs: 60\n"
        "platforms: [claude-code]\n"
        "models: [fake-model]\n"
        "judge_model: fake-model\n"
    )
    return config


def test_sdk_mocks_bind_through_real_oneharness(oneharness_env: Path) -> None:
    push = stub(pattern=r"git push( --force)?\b", output="Everything up-to-date")
    git = spy(tool="bash", pattern=r"\bgit\b")

    report = run_skill(CASE, config=oneharness_env, mocks=[push, git])

    assert report.passed, f"report: {report}"
    # The real spy JSONL round-tripped into the mock objects: original input
    # on the stub, everything observed by the spy.
    push.assert_called_once()
    assert push.calls[0].command == "git push origin main"
    assert push.calls[0].mocked
    git.assert_called_with(command=contains("git status"))
    assert git.call_count == 2
    # The canned output reached the (shimmed) model via the real responder's
    # printf rewrite, which the shim executed.
    assert "Everything up-to-date" in report.runs[0].transcript.messages[1].content
    # And real oneharness usage flowed through.
    usage = report.runs[0].usage
    assert usage is not None and (usage.input_tokens or 0) > 0


def test_sdk_spy_only_observes_through_real_oneharness(oneharness_env: Path) -> None:
    shell = spy(tool="bash")
    report = run_skill(CASE, config=oneharness_env, mocks=[shell])
    assert report.passed
    assert shell.call_count == 3
    assert all(not c.mocked for c in shell.calls), "a spy never intercepts"
    shell.where(command=contains("rm -rf")).assert_called_once()


def test_config_overrides_control_history_and_role_models(
    oneharness_env: Path, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    import json

    log = tmp_path / "calls.jsonl"
    history = tmp_path / "history"
    monkeypatch.setenv("SKILLTEST_BOUNDARY_LOG", str(log))
    report = run_skill(
        FIXTURES / "cases/greet_pass.yaml",
        config=oneharness_env,
        config_overrides={
            "provider": {
                "history": True,
                "history_dir": str(history),
                "judge": {
                    "args": [
                        "--harness",
                        "claude-code",
                        "--model",
                        "override-judge",
                        "--schema-max-retries",
                        "0",
                    ]
                },
            }
        },
    )
    assert report.passed
    assert report.runs[0].history_command and str(history) in report.runs[0].history_command
    assert list(history.rglob("*.jsonl"))
    calls = [json.loads(line) for line in log.read_text().splitlines()]
    assert [call["model"] for call in calls] == ["fake-model", "override-judge", "override-judge"]
    report = run_skill(
        CASE,
        config=oneharness_env,
        config_overrides={"provider": {"history": False, "history_dir": str(history)}},
    )
    assert report.passed and report.runs[0].history_command is None
