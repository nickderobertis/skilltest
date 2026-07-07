"""Stream a running skilltest case as an async iterator of tool events.

Opt-in on top of the buffered [`run_skill`][skilltest_sdk.runner.run_skill]:
iterate a [`SkillStream`][skilltest_sdk.stream.SkillStream] with ``async for`` to
receive each normalized tool event the instant the skill takes it, and ``break``
to **short-circuit** — the CLI subprocess is killed, which closes oneharness's
stream and tears the harness down, so a bad turn is cut off instead of paid for
in full. After the stream completes normally, ``.report`` holds the final
[`Report`].

```python
stream = stream_skill("cases/edit.skilltest.yaml")
async for ev in stream:
    if ev.event.name == "bash" and "rm -rf" in str(ev.event.input):
        break  # disallowed action — abort the run now
report = stream.report  # the full Report, when the stream ran to completion
```
"""

from __future__ import annotations

import asyncio
import contextlib
import json
from collections.abc import AsyncIterator, Sequence
from pathlib import Path

from pydantic import BaseModel

from ._error import ReportError
from ._report import ToolEvent
from .case import TestCase
from .errors import SkilltestProviderError
from .mock import ToolSpy, bind_mocks
from .models import Report
from .runner import (
    ENV_BIN,
    build_run_argv,
    case_run_args,
    child_env,
    mock_run_args,
    raise_for_code,
)


class StreamEvent(BaseModel):
    """One streamed tool event, tagged with the run it belongs to."""

    case: str
    platform: str
    model: str
    #: 1-based assistant-turn index within the run.
    turn: int
    event: ToolEvent


class SkillStream:
    """An async stream of [`StreamEvent`][skilltest_sdk.stream.StreamEvent]s from
    a running case. Iterate with ``async for``; ``break`` to short-circuit. When
    the stream runs to completion, ``report`` holds the final [`Report`]."""

    def __init__(self, argv: list[str], cwd: str | None, mocks: Sequence[ToolSpy] = ()) -> None:
        self._argv = argv
        self._cwd = cwd
        self._mocks = list(mocks)
        #: Resources (the temp mocks file) released when the stream finishes.
        self._cleanup: contextlib.ExitStack | None = None
        #: The final report, populated once the stream completes normally.
        self.report: Report | None = None

    def _close_cleanup(self) -> None:
        if self._cleanup is not None:
            self._cleanup.close()
            self._cleanup = None

    def __aiter__(self) -> AsyncIterator[StreamEvent]:
        return self._iterate()

    async def _iterate(self) -> AsyncIterator[StreamEvent]:
        try:
            proc = await asyncio.create_subprocess_exec(
                *self._argv,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
                cwd=self._cwd,
                env=child_env(),
            )
        except FileNotFoundError as exc:
            self._close_cleanup()
            raise SkilltestProviderError(
                f"could not run skilltest binary `{self._argv[0]}`: {exc}. "
                f"Set {ENV_BIN} or pass bin=..."
            ) from exc

        assert proc.stdout is not None
        try:
            structured: ReportError | None = None
            async for raw in proc.stdout:
                line = raw.decode().strip()
                if not line:
                    continue
                obj = json.loads(line)
                kind = obj.get("type")
                if kind == "event":
                    yield StreamEvent.model_validate(obj)
                elif kind == "result":
                    self.report = Report.model_validate(obj["report"])
                    if self._mocks:
                        bind_mocks(self._mocks, self.report.runs)
                elif kind == "error":
                    # The terminal error line for a failed streamed run — carries
                    # the same structured `kind`/`context` as the buffered output.
                    structured = ReportError.model_validate(obj["error"])
            await proc.wait()
            # A hard failure (bad input / provider error) once the stream ends.
            detail = ""
            if proc.stderr is not None:
                detail = (await proc.stderr.read()).decode().strip()
            raise_for_code(proc.returncode, detail, structured)
        finally:
            # The consumer stopped early (break): kill the CLI so oneharness's
            # stream closes and the harness is torn down.
            if proc.returncode is None:
                proc.kill()
                with contextlib.suppress(ProcessLookupError):
                    await proc.wait()
            self._close_cleanup()


def stream_skill(
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
) -> SkillStream:
    """Start a streaming run and return a [`SkillStream`] to iterate. Same
    arguments as [`run_skill`][skilltest_sdk.runner.run_skill] — ``case`` may be
    a code-defined [`TestCase`][skilltest_sdk.case.TestCase] or a path, and
    ``mocks`` (plus a ``TestCase``'s own mocks) bind when the stream runs to
    completion (an aborted stream leaves them unbound); the run does not begin
    until iteration starts."""
    # The temp case/mocks files must outlive the subprocess, which starts lazily
    # on iteration — materialize the flags now and keep them alive on the stream.
    case_mocks = tuple(case.mocks) if isinstance(case, TestCase) else ()
    with contextlib.ExitStack() as stack:
        case_args = stack.enter_context(case_run_args(case))
        mock_args = stack.enter_context(mock_run_args(mocks))
        argv = build_run_argv(
            case_args,
            bin=bin,
            provider=provider,
            platforms=platforms,
            models=models,
            judge_model=judge_model,
            max_turns=max_turns,
            config=config,
            fmt="json-stream",
            mock_args=mock_args,
        )
        stream = SkillStream(argv, str(cwd) if cwd is not None else None, [*case_mocks, *mocks])
        stream._cleanup = stack.pop_all()
        return stream
