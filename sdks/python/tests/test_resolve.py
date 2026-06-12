"""Unit tests for binary resolution: the precedence chain (explicit > env >
bundled wheel binary > PATH) and that a binary bundled in the wheel's ``_bin/``
is discovered when present.

A source checkout and the pure (``py3-none-any``) wheel ship no binary, so
``_bundled_bin()`` is ``None`` and the runner falls back — exactly how the e2e
suite reaches the locally built CLI via ``$SKILLTEST_BIN``.
"""

from __future__ import annotations

from pathlib import Path

import pytest

from skilltest_sdk import runner


def test_explicit_bin_wins(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv(runner.ENV_BIN, "/from/env")
    assert runner._resolve_bin("/explicit") == "/explicit"


def test_env_beats_bundled_and_path(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv(runner.ENV_BIN, "/from/env")
    assert runner._resolve_bin(None) == "/from/env"


def test_falls_back_to_path_when_unset_and_unbundled(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv(runner.ENV_BIN, raising=False)
    assert runner._bundled_bin() is None
    assert runner._resolve_bin(None) == "skilltest"


def test_bundled_binary_is_found_and_preferred(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv(runner.ENV_BIN, raising=False)
    bin_dir = Path(runner.__file__).resolve().parent / "_bin"
    bin_path = bin_dir / runner._BIN_NAME
    bin_dir.mkdir(parents=True, exist_ok=True)
    try:
        bin_path.write_text("#!/bin/sh\n")
        bin_path.chmod(0o755)
        assert runner._bundled_bin() == str(bin_path)
        assert runner._resolve_bin(None) == str(bin_path)
    finally:
        bin_path.unlink(missing_ok=True)
        if bin_dir.is_dir() and not any(bin_dir.iterdir()):
            bin_dir.rmdir()
