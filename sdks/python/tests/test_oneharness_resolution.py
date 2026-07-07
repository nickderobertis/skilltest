"""The SDK points the skilltest subprocess at the bundled `oneharness` (from the
`oneharness-cli` dependency) so a live run needs no separate install. These are
hermetic unit tests of that resolution — no real oneharness required.
"""

from __future__ import annotations

import os
from pathlib import Path

import pytest

from skilltest_sdk import runner
from skilltest_sdk.runner import ENV_ONEHARNESS_BIN, _bundled_oneharness, child_env


def _fake_scripts_dir(monkeypatch: pytest.MonkeyPatch, tmp_path: Path, present: bool) -> Path:
    """Point `sysconfig.get_path("scripts")` at a temp dir, optionally holding a
    fake `oneharness` console entry (as `oneharness-cli` would install)."""
    if present:
        (tmp_path / runner._ONEHARNESS_NAME).write_text("#!/bin/sh\n")
    monkeypatch.setattr(runner.sysconfig, "get_path", lambda name: str(tmp_path))
    return tmp_path


def test_bundled_oneharness_found_in_scripts_dir(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    scripts = _fake_scripts_dir(monkeypatch, tmp_path, present=True)
    assert _bundled_oneharness() == str(scripts / runner._ONEHARNESS_NAME)


def test_bundled_oneharness_absent_returns_none(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    _fake_scripts_dir(monkeypatch, tmp_path, present=False)
    assert _bundled_oneharness() is None


def test_child_env_points_at_bundled_oneharness(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    scripts = _fake_scripts_dir(monkeypatch, tmp_path, present=True)
    monkeypatch.delenv(ENV_ONEHARNESS_BIN, raising=False)
    env = child_env()
    assert env is not None
    assert env[ENV_ONEHARNESS_BIN] == str(scripts / runner._ONEHARNESS_NAME)
    # The rest of the parent environment is carried through unchanged.
    assert env["PATH"] == os.environ["PATH"]


def test_child_env_honors_user_set_var(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    _fake_scripts_dir(monkeypatch, tmp_path, present=True)
    monkeypatch.setenv(ENV_ONEHARNESS_BIN, "/my/oneharness")
    # Already chosen by the caller: inherit the env unchanged (None) rather than
    # override it with the bundled one.
    assert child_env() is None


def test_child_env_inherits_when_no_bundled(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    _fake_scripts_dir(monkeypatch, tmp_path, present=False)
    monkeypatch.delenv(ENV_ONEHARNESS_BIN, raising=False)
    assert child_env() is None
