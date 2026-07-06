"""The mock/spy API, driven through the real binary + fake provider: construct
mocks/spies as variables, pass them to ``run_skill``, assert on them directly.

The deployer fixture skill scripts three tool calls (`git push origin main`,
`git status`, `rm -rf /tmp/build`), so interception and observation are
deterministic.
"""

from __future__ import annotations

from pathlib import Path

import pytest

from skilltest_sdk import (
    SkilltestUsageError,
    anything,
    contains,
    deny,
    matching,
    rewrite,
    run_skill,
    spy,
    stub,
)
from skilltest_sdk.mock import compile_decls


def deploy_case(cases: Path) -> Path:
    return cases / "deploy_plain.yaml"


def test_stub_intercepts_and_binds_original_calls(cases: Path) -> None:
    push = stub(pattern=r"git push( --force)?\b", output="Everything up-to-date")
    git = spy(tool="bash", pattern=r"\bgit\b")

    report = run_skill(deploy_case(cases), mocks=[push, git])

    assert report.passed
    push.assert_called_once()
    # The mock's call carries the ORIGINAL input, not the substituted stub.
    assert push.calls[0].command == "git push origin main"
    assert push.calls[0].mocked
    assert push.calls[0].platform == report.runs[0].platform
    # The spy observes everything matching — including the mocked push.
    assert git.call_count == 2
    git.assert_called_with(command=contains("git status"))
    # And the canned output surfaced to the model.
    assert "Everything up-to-date" in report.runs[0].transcript.messages[1].content


def test_deny_blocks_and_where_filters(cases: Path) -> None:
    danger = deny(contains="rm -rf", message="blocked in tests")
    shell = spy(tool="bash")

    run_skill(deploy_case(cases), mocks=[danger, shell])

    danger.assert_called_once()
    assert danger.calls[0].action == "deny"
    # where(): the filtered view carries the full assertion surface, and an
    # empty result is a first-class citizen.
    shell.where(command=matching(r"\bgit\b")).assert_called_times(2)
    shell.where(command=contains("sudo")).assert_not_called()
    shell.where(tool="bash", command=anything()).assert_called_times(3)
    with pytest.raises(AssertionError, match="observed:"):
        shell.where(command=contains("sudo")).assert_called()


def test_rewrite_substitutes_input(cases: Path) -> None:
    redirect = rewrite(contains="git status", input={"command": "true"})
    report = run_skill(deploy_case(cases), mocks=[redirect])
    redirect.assert_called_once()
    # Original intent on the mock; post-rewrite reality in the transcript.
    assert redirect.calls[0].command == "git status"
    events = report.runs[0].transcript.messages[1].events or []
    assert any(e.input == {"command": "true"} for e in events)


def test_unbound_access_raises_never_reads_as_zero() -> None:
    lonely = spy(tool="bash")
    with pytest.raises(SkilltestUsageError, match="not bound"):
        _ = lonely.calls
    with pytest.raises(SkilltestUsageError, match="not bound"):
        lonely.assert_not_called()
    with pytest.raises(SkilltestUsageError, match="not bound"):
        lonely.where(command=contains("x"))


def test_each_run_rebinds_fresh(cases: Path) -> None:
    git = spy(tool="bash", pattern=r"\bgit\b")
    run_skill(deploy_case(cases), mocks=[git])
    first = git.call_count
    run_skill(deploy_case(cases), mocks=[git])
    assert git.call_count == first, "re-binding replaces, never accumulates"


def test_assertion_failures_show_the_observed_calls(cases: Path) -> None:
    push = stub(contains="git push", output="ok")
    run_skill(deploy_case(cases), mocks=[push])
    with pytest.raises(AssertionError) as excinfo:
        push.assert_called_times(3)
    message = str(excinfo.value)
    assert "expected exactly 3" in message
    assert "git push origin main" in message, "the actual calls are in the failure"


def test_spy_criteria_are_validated_at_construction() -> None:
    with pytest.raises(SkilltestUsageError, match="at least one criterion"):
        spy()
    with pytest.raises(SkilltestUsageError, match="match everything"):
        spy(contains="")
    with pytest.raises(SkilltestUsageError, match="non-empty"):
        contains("")


def test_mock_criteria_must_compile_to_the_hook_side() -> None:
    # A predicate cannot run inside the harness: loud at construction-compile,
    # never a silently narrower mock.
    predicate_mock = stub(tool="bash", where={"command": lambda v: "x" in v}, output="y")
    with pytest.raises(SkilltestUsageError, match="cannot be compiled"):
        compile_decls([predicate_mock])
    # anything() is spy-only for the same reason.
    exists_mock = deny(tool="bash", where={"command": anything()}, message="no")
    with pytest.raises(SkilltestUsageError, match="cannot be compiled"):
        compile_decls([exists_mock])
    # The shipped matchers and exact strings compile fine.
    ok = stub(
        tool="bash",
        where={"command": contains("git"), "flag": matching(r"^-"), "mode": "fast"},
        output="y",
    )
    (decl,) = compile_decls([ok])
    assert decl["match"]["input"] == {
        "command": {"contains": "git"},
        "flag": {"pattern": "^-"},
        "mode": "fast",
    }
    assert decl["stub"] == {"output": "y", "exit_code": 0}


def test_spy_accepts_predicates_locally(cases: Path) -> None:
    # Spies filter locally, so arbitrary predicates are fine.
    forced = spy(tool="bash", where={"command": lambda v: v.startswith("git")})
    run_skill(deploy_case(cases), mocks=[forced])
    assert forced.call_count == 2
