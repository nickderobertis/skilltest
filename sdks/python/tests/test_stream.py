"""Streaming-API e2e tests against the built binary + fake provider."""

from __future__ import annotations

import asyncio
from pathlib import Path

import pytest

from skilltest_sdk import (
    Report,
    SkilltestProviderError,
    SkilltestTimeoutError,
    StreamEvent,
    stream_skill,
)


def _stream_fake_oneharness_config(tmp_path: Path, status: str) -> Path:
    """A config pointing the oneharness provider at a fake that speaks the
    ``run --stream`` NDJSON protocol and ends in a failed result, so the
    streaming failure path can be exercised offline."""
    oh = tmp_path / "oneharness"
    results = f'{{"results":[{{"status":"{status}","stderr":"deadline exceeded"}}]}}'
    line = f'{{"type":"result","report":{results}}}'
    oh.write_text(f"#!/bin/sh\ncat >/dev/null\nprintf '%s\\n' '{line}'\n")
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


def test_stream_yields_events_then_exposes_report(cases: Path) -> None:
    async def go() -> tuple[list[str | None], Report | None]:
        stream = stream_skill(cases / "tool_events.yaml")
        names: list[str | None] = []
        async for ev in stream:
            assert isinstance(ev, StreamEvent)
            assert ev.case == "tool_events"
            assert ev.turn == 1
            names.append(ev.event.name)
        return names, stream.report

    names, report = asyncio.run(go())
    assert names == ["edit_file", "bash"]
    assert report is not None
    assert report.passed


def test_stream_short_circuits_on_break(cases: Path) -> None:
    async def go() -> int:
        stream = stream_skill(cases / "tool_events.yaml")
        seen = 0
        async for _ in stream:
            seen += 1
            break  # abort after the first event
        return seen

    assert asyncio.run(go()) == 1


def test_stream_raises_kind_specific_error_on_provider_failure(
    cases: Path, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    # A provider failure during a streamed run surfaces the same kind-specific
    # exception as the buffered API: the terminal `{"type":"error",…}` NDJSON
    # line carries the classified kind, and the stream raises it once drained.
    monkeypatch.delenv("SKILLTEST_PROVIDER", raising=False)
    cfg = _stream_fake_oneharness_config(tmp_path, "timeout")

    async def go() -> None:
        stream = stream_skill(cases / "greet_pass.yaml", config=cfg)
        async for _ in stream:
            pass

    with pytest.raises(SkilltestTimeoutError) as exc:
        asyncio.run(go())
    assert isinstance(exc.value, SkilltestProviderError)
    assert exc.value.kind == "timeout"
    assert exc.value.context == "oneharness:claude-code"
