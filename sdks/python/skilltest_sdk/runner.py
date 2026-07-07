"""Run the ``skilltest`` CLI as a subprocess and parse its JSON contract.

This is the code-level API: call [`run_skill`][skilltest_sdk.runner.run_skill],
get a typed [`Report`], assert on ``report.passed``, and mix in any
deterministic checks against the transcript.
"""

from __future__ import annotations

import contextlib
import json
import os
import subprocess
import tempfile
from collections.abc import Iterator, Sequence
from pathlib import Path

from pydantic import BaseModel, ValidationError

from ._error import ReportError
from .case import TestCase
from .errors import SkilltestError, SkilltestProviderError, SkilltestUsageError
from .mock import ToolSpy, bind_mocks, compile_decls
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
    case: str | Path | TestCase,
    *,
    bin: str | Path | None = None,
    provider: str | Sequence[str] | None = None,
    platforms: Sequence[str] = (),
    models: Sequence[str] = (),
    judge_model: str | None = None,
    max_turns: int | None = None,
    config: str | Path | None = None,
    cwd: str | Path | None = None,
    mocks: Sequence[ToolSpy] = (),
) -> Report:
    """Run one or more test cases and return the parsed [`Report`].

    ``case`` is a [`TestCase`][skilltest_sdk.case.TestCase] built in code (the
    recommended form), a test-case YAML file, or a directory of them. A failing
    eval is *not* an exception — it is reported in
    ``report.passed``/``report.runs`` so the caller can assert and inspect. Only
    bad input ([`SkilltestUsageError`]) and provider failures
    ([`SkilltestProviderError`]) raise.

    ``mocks`` takes [`spy`][skilltest_sdk.mock.spy] /
    [`stub`][skilltest_sdk.mock.stub] / [`deny`][skilltest_sdk.mock.deny] /
    [`rewrite`][skilltest_sdk.mock.rewrite] objects: mocks compile into the
    run's hook-side ruleset (prepended to the case's own `mocks:` block, so
    the test-local rule wins), and after the run every object is bound with
    the calls it matched — assert on it directly. A ``TestCase``'s own
    ``mocks`` are bound the same way. Each call re-binds fresh.
    """
    case_mocks = tuple(case.mocks) if isinstance(case, TestCase) else ()
    with case_run_args(case) as case_args, mock_run_args(mocks) as mock_args:
        argv = build_run_argv(
            case_args,
            bin=bin,
            provider=provider,
            platforms=platforms,
            models=models,
            judge_model=judge_model,
            max_turns=max_turns,
            config=config,
            fmt="json",
            mock_args=mock_args,
        )
        proc = _run(argv, cwd)
    _raise_for_status(proc)
    report = _parse(Report, proc.stdout)
    bound = [*case_mocks, *mocks]
    if bound:
        bind_mocks(bound, report.runs)
    return report


@contextlib.contextmanager
def case_run_args(case: str | Path | TestCase) -> Iterator[list[str]]:
    """The ``skilltest run`` argv fragments a ``case`` becomes (internal, shared
    with the streaming API): a YAML file/directory rides as a positional path; a
    code-defined [`TestCase`][skilltest_sdk.case.TestCase] is written to a temp
    JSON file passed as ``--case-json``. The temp file lives for the ``with``
    body (the run)."""
    if not isinstance(case, TestCase):
        yield [str(case)]
        return
    fd, path = tempfile.mkstemp(prefix="skilltest-case-", suffix=".json")
    try:
        with os.fdopen(fd, "w") as handle:
            json.dump([case._compile()], handle)
        yield ["--case-json", path]
    finally:
        with contextlib.suppress(OSError):
            os.unlink(path)


@contextlib.contextmanager
def mock_run_args(mocks: Sequence[ToolSpy]) -> Iterator[list[str]]:
    """The CLI flags a ``mocks=`` argument turns into (internal, shared with
    the streaming API): ``--spy`` so the observation channel is on for spies,
    plus ``--mocks <tempfile>`` carrying the compiled mock declarations. The
    temp file lives for the duration of the ``with`` body (the run)."""
    if not mocks:
        yield []
        return
    args = ["--spy"]
    decls = compile_decls(mocks)
    if not decls:
        yield args
        return
    fd, path = tempfile.mkstemp(prefix="skilltest-mocks-", suffix=".json")
    try:
        with os.fdopen(fd, "w") as handle:
            json.dump(decls, handle)
        yield [*args, "--mocks", path]
    finally:
        with contextlib.suppress(OSError):
            os.unlink(path)


def build_run_argv(
    case_args: Sequence[str],
    *,
    bin: str | Path | None,
    provider: str | Sequence[str] | None,
    platforms: Sequence[str],
    models: Sequence[str],
    judge_model: str | None,
    max_turns: int | None,
    config: str | Path | None,
    fmt: str,
    mock_args: Sequence[str] = (),
) -> list[str]:
    """Build the ``skilltest run`` argv for output format ``fmt`` (``json`` for the
    buffered API, ``json-stream`` for the streaming API). ``case_args`` are the
    fragments identifying the case(s) — a positional path or ``--case-json
    <file>`` (see [`case_run_args`][skilltest_sdk.runner.case_run_args]).
    Internal, shared by ``run_skill`` and the streaming API."""
    argv = [_resolve_bin(bin)]
    if config is not None:
        argv += ["--config", str(config)]
    argv += ["run", *case_args, "--format", fmt]

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
    argv += list(mock_args)
    return argv


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
    structured = parse_report_error(proc.stdout)
    raise_for_code(proc.returncode, proc.stderr.strip() or proc.stdout.strip(), structured)


def parse_report_error(stdout: str) -> ReportError | None:
    """The structured error the CLI emits on stdout for a ``--format json``
    failure, or ``None`` when stdout is not that envelope — an older binary that
    printed nothing to stdout, or a human-format run. Callers fall back to the
    stderr text. Shared by the buffered and streaming APIs."""
    text = stdout.strip()
    if not text:
        return None
    try:
        return ReportError.model_validate_json(text)
    except ValidationError:
        return None


def raise_for_code(
    code: int | None,
    detail: str,
    structured: ReportError | None = None,
) -> None:
    """Map a skilltest exit code to an exception (shared by the buffered and
    streaming APIs). Codes 0/1 produce a report and never raise. ``structured``,
    when the CLI emitted the JSON error envelope, carries the classified
    ``kind``/``context`` onto a [`SkilltestProviderError`]."""
    if code in _REPORTING_CODES:
        return
    message = (structured.message if structured else "") or detail
    if code == 2:
        raise SkilltestUsageError(message)
    if code == 3:
        kind = structured.kind if structured else None
        context = structured.context if structured else None
        raise SkilltestProviderError(message, kind=kind, context=context)
    raise SkilltestError(f"skilltest exited {code}: {message}")


def _parse[T: BaseModel](model: type[T], stdout: str) -> T:
    try:
        return model.model_validate_json(stdout)
    except ValidationError as exc:
        raise SkilltestError(f"skilltest output did not match the expected schema: {exc}") from exc
