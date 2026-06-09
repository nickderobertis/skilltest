"""Shared test setup: point the plugin at the locally built Rust binaries.

These tests exercise the *real* `skilltest` binary and the deterministic
`skilltest-fake-provider`, both built by `cargo build` (run via `just bootstrap`
/ `just check` before the Python suite). Only the model is faked.
"""

from __future__ import annotations

import os
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parents[3]
TARGET = REPO_ROOT / "target" / "debug"
SKILLTEST_BIN = TARGET / "skilltest"
FAKE_PROVIDER = TARGET / "skilltest-fake-provider"
FIXTURES = REPO_ROOT / "tests" / "fixtures"

# Defaults so both the code-API tests and the auto-collected case file find the
# binary and provider without per-call wiring.
os.environ.setdefault("SKILLTEST_BIN", str(SKILLTEST_BIN))
os.environ.setdefault("SKILLTEST_PROVIDER", str(FAKE_PROVIDER))


@pytest.fixture(scope="session", autouse=True)
def _require_binaries() -> None:
    missing = [p for p in (SKILLTEST_BIN, FAKE_PROVIDER) if not p.exists()]
    if missing:
        names = ", ".join(str(p) for p in missing)
        pytest.fail(
            f"built binaries not found: {names}. Run `just bootstrap` (cargo build) first.",
            pytrace=False,
        )


@pytest.fixture
def fixtures() -> Path:
    return FIXTURES


@pytest.fixture
def cases(fixtures: Path) -> Path:
    return fixtures / "cases"
