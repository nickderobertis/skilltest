"""Streaming-API e2e tests against the built binary + fake provider."""

from __future__ import annotations

import asyncio
from pathlib import Path

from skilltest_sdk import Report, StreamEvent, stream_skill


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
