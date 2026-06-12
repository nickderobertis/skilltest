"""pytest integration: collect ``*.skilltest.yaml`` files as test items.

Drop a ``greets.skilltest.yaml`` next to your other tests and `pytest` will run
it as a case, failing with the judge's reasons when an eval does not pass. For
finer control — multiple platforms/models, or deterministic mix-in assertions on
the transcript — call [`run_skill`][skilltest_sdk.runner.run_skill] from an
ordinary test function instead.

Settings come from ``pytest.ini``/``pyproject.toml`` (``skilltest_bin``,
``skilltest_provider``, ``skilltest_platforms``, ``skilltest_models``,
``skilltest_config``) or the ``SKILLTEST_BIN`` / ``SKILLTEST_PROVIDER`` env vars.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

import pytest
from skilltest_sdk import Report, describe_failures, run_skill

if TYPE_CHECKING:
    from collections.abc import Sequence

_SUFFIXES = (".skilltest.yaml", ".skilltest.yml")


def pytest_addoption(parser: pytest.Parser) -> None:
    parser.addini("skilltest_bin", "Path to the skilltest binary", default=None)
    parser.addini("skilltest_provider", "Provider command for skilltest", default=None)
    parser.addini("skilltest_platforms", "Platforms to run cases on", type="args", default=[])
    parser.addini("skilltest_models", "Models to run cases on", type="args", default=[])
    parser.addini("skilltest_config", "Path to a skilltest config file", default=None)


def pytest_collect_file(parent: pytest.Collector, file_path) -> SkilltestFile | None:
    name = file_path.name
    if any(name.endswith(suffix) for suffix in _SUFFIXES):
        return SkilltestFile.from_parent(parent, path=file_path)
    return None


class _Settings:
    """Resolved collector settings, read once from the pytest config."""

    def __init__(self, config: pytest.Config) -> None:
        self.bin: str | None = config.getini("skilltest_bin") or None
        self.provider: str | None = config.getini("skilltest_provider") or None
        self.platforms: Sequence[str] = config.getini("skilltest_platforms")
        self.models: Sequence[str] = config.getini("skilltest_models")
        self.config: str | None = config.getini("skilltest_config") or None


class SkilltestFailure(Exception):
    """Raised when a collected case fails, carrying the report for reporting."""

    def __init__(self, report: Report) -> None:
        super().__init__(describe_failures(report))
        self.report = report


class SkilltestFile(pytest.File):
    def collect(self):  # type: ignore[override]
        yield SkilltestItem.from_parent(self, name=self.path.stem)


class SkilltestItem(pytest.Item):
    def runtest(self) -> None:
        settings = _Settings(self.config)
        report = run_skill(
            self.path,
            bin=settings.bin,
            provider=settings.provider,
            platforms=settings.platforms,
            models=settings.models,
            config=settings.config,
        )
        if not report.passed:
            raise SkilltestFailure(report)

    def repr_failure(self, excinfo, style=None):  # type: ignore[override]
        if isinstance(excinfo.value, SkilltestFailure):
            return f"skilltest case failed:\n{excinfo.value}"
        return super().repr_failure(excinfo, style=style)

    def reportinfo(self):  # type: ignore[override]
        return self.path, 0, f"skilltest: {self.name}"
