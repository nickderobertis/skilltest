#!/usr/bin/env python3
"""Render the animated demo GIF of a typical `skilltest run` (the README hero).

Like `scripts/screenshots.sh`, this drives the **real release `skilltest` binary**
against the bundled deterministic `skilltest-fake-provider` and the dedicated
fixtures under `screenshots/fixture/` — so the cases, evals, and final report are
genuine CLI output, only the model is faked (no harness, no network, no cost).
The CLI prints its report all at once, not as a live view, so instead of
capturing a PTY (which would need ttyd/ffmpeg) we reconstruct the end-to-end run
the way a viewer experiences it — each case resolving from queued to
passed/failed as its evals return, then clearing to reveal the genuine report —
from a real run's structured output, and render the frames with the **vendored,
pinned JetBrains Mono font** (`screenshots/fonts/`, the same one the SVG
screenshots use). The result is deterministic and self-contained (Pillow only).

The GIF is informational, like the screenshots — it is NOT hash-gated (a GIF is
not byte-reproducible across Pillow versions), so it is regenerated on demand with
`just screenshots-gif` and committed to `docs/screenshots/demo.gif`.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

# GitHub-dark palette, matching the SVG screenshots' window (bg #0d1117).
BG = (13, 17, 23)
BAR = (22, 27, 34)
FG = (201, 209, 217)
DIM = (139, 148, 158)
GREEN = (63, 185, 80)
RED = (248, 81, 73)
YELLOW = (210, 153, 34)
CYAN = (57, 197, 207)
DOTS = [(255, 95, 86), (255, 189, 46), (39, 201, 63)]  # traffic-light window dots

# A rotating quadrant-block spinner. The vendored JetBrains Mono renders these as
# four *distinct* glyphs (unlike braille or quadrant circles, which collapse to a
# single fallback glyph at this size — making the animation freeze).
SPINNER = "▖▘▝▗"
MIN_COLS = 56       # a floor so the window never looks cramped
FONT_SIZE = 20
PAD = 24
BAR_H = 40
FRAME_MS = 130      # per animation frame
HOLD_MS = 2800      # hold on the final report


def _run(binp: str, fake: str, fixture: str, extra: list[str]) -> subprocess.CompletedProcess:
    """Drive the real binary against the fixture cases with the fake provider."""
    return subprocess.run(
        [binp, "run", "cases", "--provider", fake,
         "--platform", "demo", "--model", "fake", *extra],
        cwd=fixture, env=dict(os.environ), capture_output=True, text=True,
    )


def run_json(binp: str, fake: str, fixture: str) -> dict:
    """Run the real binary against the fixture cases and return the full report."""
    return json.loads(_run(binp, fake, fixture, ["--format", "json"]).stdout)


def run_report(binp: str, fake: str, fixture: str) -> list[str]:
    """The genuine plain-text final report (what the animation clears to reveal)."""
    return _run(binp, fake, fixture, []).stdout.splitlines()


# A frame is a list of lines; a line is a list of (text, color) segments.
def build_frames(report: dict, human: list[str]) -> list[tuple[list, int]]:
    runs = report["runs"]
    order = sorted(runs, key=lambda r: r["case"])
    total = len(order)

    frames: list[tuple[list, int]] = []
    done: dict[str, bool] = {}
    running: str | None = None
    spin = 0

    def render(spin_i: int) -> list:
        sp = SPINNER[spin_i % len(SPINNER)]
        finished = len(done)
        lines = [[(f"{sp} skilltest run cases  ({finished}/{total} cases)", CYAN)]]
        lines.append([("", FG)])
        for r in order:
            name = r["case"]
            label = f"{name} [demo/fake]"
            if name in done:
                if done[name]:
                    lines.append([("✓ ", GREEN), (label, FG), ("  PASS", GREEN)])
                else:
                    lines.append([("✗ ", RED), (label, FG), ("  FAIL", RED)])
            elif name == running:
                lines.append([(f"{sp} ", CYAN), (label, FG), ("  running", CYAN)])
            else:
                lines.append([(f"{sp} ", CYAN), (label, DIM), ("  queued", DIM)])
        return lines

    # Opening beat: everything queued.
    for _ in range(3):
        frames.append((render(spin), FRAME_MS))
        spin += 1

    # Each case runs, then resolves to its real pass/fail.
    for r in order:
        running = r["case"]
        for _ in range(3):
            frames.append((render(spin), FRAME_MS))
            spin += 1
        running = None
        done[r["case"]] = bool(r["passed"])
        frames.append((render(spin), FRAME_MS))
        spin += 1

    # Settle on the fully-resolved view briefly...
    for _ in range(3):
        frames.append((render(spin), FRAME_MS))
        spin += 1

    # ...then the view clears and the genuine report is revealed.
    frames.append(([colorize_report(line) for line in human], HOLD_MS))
    return frames


def colorize_report(line: str) -> list:
    """Colorize a plain report line the way a colorized human report would."""
    if line.startswith("FAIL") or line.startswith("ERROR"):
        head, _, rest = line.partition(" ")
        return [(head, RED), (" " + rest, FG)]
    if line.startswith("PASS") or line.startswith("OK"):
        head, _, rest = line.partition(" ")
        return [(head, GREEN), (" " + rest, FG)]
    if line.lstrip().startswith("-"):
        return [(line, DIM)]
    if "runs passed" in line:
        return [(line, FG)]
    return [(line, FG)]


def render_gif(frames: list[tuple[list, int]], font_path: str, out: str) -> None:
    font = ImageFont.truetype(font_path, FONT_SIZE)
    cw = int(font.getlength("M"))
    asc, desc = font.getmetrics()
    lh = asc + desc + 6
    rows = max(len(f[0]) for f in frames)
    # Size the window to the widest line any frame draws (the final report's
    # itemized failing eval), so nothing is ever clipped — with a floor.
    cols = max(
        MIN_COLS,
        max(sum(len(text) for text, _ in line) for lines, _ in frames for line in lines),
    )
    width = PAD * 2 + cols * cw
    height = BAR_H + PAD + rows * lh + PAD

    def draw_frame(lines: list) -> Image.Image:
        img = Image.new("RGB", (width, height), BG)
        d = ImageDraw.Draw(img)
        # Window chrome: a title bar with three traffic-light dots.
        d.rectangle([0, 0, width, BAR_H], fill=BAR)
        for i, col in enumerate(DOTS):
            cx = PAD + i * 22
            cy = BAR_H // 2
            d.ellipse([cx - 6, cy - 6, cx + 6, cy + 6], fill=col)
        y = BAR_H + PAD
        for segs in lines:
            x = PAD
            for text, color in segs:
                d.text((x, y), text, font=font, fill=color)
                x += int(font.getlength(text))
            y += lh
        return img

    imgs = [draw_frame(lines) for lines, _ in frames]
    durations = [ms for _, ms in frames]
    imgs[0].save(
        out, save_all=True, append_images=imgs[1:], duration=durations,
        loop=0, optimize=True, disposal=2,
    )


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    binp = os.environ.get("SKILLTEST_BIN", str(root / "target/release/skilltest"))
    fake = os.environ.get(
        "SKILLTEST_FAKE_BIN", str(root / "target/release/skilltest-fake-provider")
    )
    fixture = str(root / "screenshots/fixture")
    font_path = str(root / "screenshots/fonts/JetBrainsMono-Regular.ttf")
    out = os.environ.get("DEMO_GIF_OUT", str(root / "docs/screenshots/demo.gif"))

    for p in (binp, fake, font_path):
        if not Path(p).exists():
            print(f"demo-gif: missing {p}", file=sys.stderr)
            return 1

    report = run_json(binp, fake, fixture)
    human = run_report(binp, fake, fixture)
    frames = build_frames(report, human)
    Path(out).parent.mkdir(parents=True, exist_ok=True)
    render_gif(frames, font_path, out)
    print(f"demo-gif: wrote {out} ({len(frames)} frames)", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
