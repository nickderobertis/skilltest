"""Run the ``skilltest`` CLI as a subprocess and parse its JSON contract.

This is the code-level API: call [`run_skill`][skilltest_sdk.runner.run_skill],
get a typed [`Report`], assert on ``report.passed``, and mix in any
deterministic checks against the transcript.
"""

from __future__ import annotations

import contextlib
import os
import subprocess
from collections.abc import Sequence
from pathlib import Path

from pydantic import BaseModel, ValidationError

from .errors import SkilltestError, SkilltestProviderError, SkilltestUsageError
from .models import Report, ValidationReport

#: Environment variables that supply defaults so callers (test-framework
#: packages, CI) can locate the binary and provider without per-call arguments.
ENV_BIN = "SKILLTEST_BIN"
ENV_PROVIDER = "SKILLTEST_PROVIDER"

# Exit codes that still produce a JSON report (0 = all passed, 1 = some failed).
_REPORTING_CODES = frozenset({0, 1})


#: Name of the bundled binary inside the wheel's ``_bin/`` directory.
_BIN_NAME = "skilltest.exe" if os.name == "nt" else "skilltest"


def _bundled_bin() -> str | None:
    """Path to the binary bundled in this wheel, or ``None`` when absent.

    Platform wheels ship the prebuilt CLI at ``skilltest_sdk/_bin/skilltest``;
    the pure (``py3-none-any``) wheel and a source checkout ship none, so callers
    fall back to ``$SKILLTEST_BIN``/``PATH``. Wheel packing can drop the
    executable bit, so restore it best-effort before handing back the path.
    """
    candidate = Path(__file__).resolve().parent / "_bin" / _BIN_NAME
    if not candidate.is_file():
        return None
    if not os.access(candidate, os.X_OK):
        with contextlib.suppress(OSError):
            candidate.chmod(0o755)
    return str(candidate)


def _resolve_bin(bin: str | Path | None) -> str:
    """Resolve the binary, most explicit first: an explicit ``bin``, then
    ``$SKILLTEST_BIN``, then the binary bundled in a platform wheel, then
    ``skilltest`` on ``PATH``."""
    if bin is not None:
        return str(bin)
    env = os.environ.get(ENV_BIN)
    if env:
        return env
    return _bundled_bin() or "skilltest"


def _resolve_provider(provider: str | Sequence[str] | None) -> str | None:
    if provider is None:
        provider = os.environ.get(ENV_PROVIDER)
    if provider is None:
        return None
    if isinstance(provider, str):
        return provider
    return " ".join(provider)


def _run(argv: list[str], cwd: str | Path | None) -> subprocess.CompletedProcess[str]:
    try:
        return subprocess.run(
            argv,
            capture_output=True,
            text=True,
            cwd=cwd,
            check=False,
        )
    except FileNotFoundError as exc:
        raise SkilltestProviderError(
            f"could not run skilltest binary `{argv[0]}`: {exc}. Set {ENV_BIN} or pass bin=..."
        ) from exc


def run_skill(
    case: str | Path,
    *,
    bin: str | Path | None = None,
    provider: str | Sequence[str] | None = None,
    platforms: Sequence[str] = (),
    models: Sequence[str] = (),
    judge_model: str | None = None,
    max_turns: int | None = None,
    config: str | Path | None = None,
    cwd: str | Path | None = None,
) -> Report:
    """Run one or more test cases and return the parsed [`Report`].

    ``case`` is a test-case YAML file or a directory of them. A failing eval is
    *not* an exception — it is reported in ``report.passed``/``report.runs`` so
    the caller can assert and inspect. Only bad input ([`SkilltestUsageError`])
    and provider failures ([`SkilltestProviderError`]) raise.
    """
    argv = [_resolve_bin(bin)]
    if config is not None:
        argv += ["--config", str(config)]
    argv += ["run", str(case), "--format", "json"]

    resolved_provider = _resolve_provider(provider)
    if resolved_provider is not None:
        argv += ["--provider", resolved_provider]
    for platform in platforms:
        argv += ["--platform", platform]
    for model in models:
        argv += ["--model", model]
    if judge_model is not None:
        argv += ["--judge-model", judge_model]
    if max_turns is not None:
        argv += ["--max-turns", str(max_turns)]

    proc = _run(argv, cwd)
    _raise_for_status(proc)
    return _parse(Report, proc.stdout)


def validate_skill(
    path: str | Path,
    *,
    bin: str | Path | None = None,
    cwd: str | Path | None = None,
) -> ValidationReport:
    """Validate a skill directory (or a folder of them) and return findings."""
    argv = [_resolve_bin(bin), "validate", str(path), "--format", "json"]
    proc = _run(argv, cwd)
    _raise_for_status(proc)
    return _parse(ValidationReport, proc.stdout)


def _raise_for_status(proc: subprocess.CompletedProcess[str]) -> None:
    if proc.returncode in _REPORTING_CODES:
        return
    detail = proc.stderr.strip() or proc.stdout.strip()
    if proc.returncode == 2:
        raise SkilltestUsageError(detail)
    if proc.returncode == 3:
        raise SkilltestProviderError(detail)
    raise SkilltestError(f"skilltest exited {proc.returncode}: {detail}")


def _parse[T: BaseModel](model: type[T], stdout: str) -> T:
    try:
        return model.model_validate_json(stdout)
    except ValidationError as exc:
        raise SkilltestError(f"skilltest output did not match the expected schema: {exc}") from exc
