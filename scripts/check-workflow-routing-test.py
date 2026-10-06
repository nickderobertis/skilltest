#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["pyyaml==6.0.2"]
# ///
# llmlint: ignore-file[new_code_lands_in_a_project] tests scripts/check-workflow-routing.py, a repo-level gate that belongs to no Nx project; run workspace-wide from `just workflows-check` (AGENTS.md: scripts/* are orchestrator-independent glue).
"""Test of scripts/check-workflow-routing.py, driven as a subprocess.

The checker's own mutations prove its routing assertions can fail; this proves
its command boundary: green on a copy of the committed workflows, exit 1 naming
the drift when a copy breaks the routing or the toolchain falls behind a build
matrix, and exit 2 on input it cannot model — a malformed workflow, a malformed
`if:` expression, unreadable TOML — or an unknown argument. Quiet on success,
one line.
"""

from __future__ import annotations

import shutil
import subprocess
import sys
import tempfile
from collections.abc import Callable
from pathlib import Path
from typing import NamedTuple

ROOT = Path(__file__).resolve().parent.parent
CHECKER = ROOT / "scripts" / "check-workflow-routing.py"
FIX = "fix scripts/check-workflow-routing.py (or this test, if the contract changed on purpose)"


class Case(NamedTuple):
    label: str
    edit: Callable[[Path, Path], None]  # (workflows dir, toolchain file) -> None, edits the scratch copies
    exit_code: int
    needle: str
    extra_args: tuple[str, ...] = ()


def _replace(path: Path, old: str, new: str) -> None:
    text = path.read_text()
    if old not in text:
        raise SystemExit(
            f"check-workflow-routing-test: {path.name} no longer contains {old!r}; update this test's edit"
        )
    path.write_text(text.replace(old, new, 1))


def _unchanged(_wf: Path, _tc: Path) -> None:
    return None


CASES = (
    Case("the committed workflows", _unchanged, 0, "route as the release model requires"),
    Case("an unknown argument", _unchanged, 2, "unrecognized arguments: --bogus", ("--bogus",)),
    Case(
        "a live suite triggered on pull_request",
        lambda wf, _tc: _replace(wf / "e2e-claude.yml", "  workflow_call:\n", "  workflow_call:\n  pull_request:\n"),
        1,
        "live suite e2e-claude.yml is triggered",
    ),
    Case(
        "the toolchain missing a release target",
        lambda _wf, tc: _replace(tc, '  "x86_64-apple-darwin",\n', ""),
        1,
        "x86_64-apple-darwin is built by",
    ),
    Case(
        "a workflow whose jobs are a list",
        lambda wf, _tc: (wf / "notignored.yml").write_text("on: pull_request\njobs: [a, b]\n"),
        2,
        "notignored.yml: jobs must be a mapping",
    ),
    Case(
        "an if: with an unclosed parenthesis",
        lambda wf, _tc: _replace(wf / "pr-title.yml", "if: github.repository ==", "if: (github.repository =="),
        2,
        "expected ')'",
    ),
    Case(
        "an if: calling an unsupported function",
        lambda wf, _tc: _replace(wf / "pr-title.yml", "if: github.repository ==", "if: contains(github.ref) ||"),
        2,
        "unsupported function contains()",
    ),
    Case("an unreadable toolchain", lambda _wf, tc: tc.write_text("[toolchain\n"), 2, "cannot model the workflows"),
    Case(
        "a trigger filter the model does not simulate",
        lambda wf, _tc: _replace(
            wf / "ci.yml", "    branches: [main]\n", "    branches: [main]\n    paths: [src/**]\n"
        ),
        2,
        "on.push.paths is not a filter this model simulates",
    ),
    Case(
        "a matrix the model does not expand",
        lambda wf, _tc: _replace(
            wf / "ci.yml", "os: [ubuntu-latest, macos-latest]", "os: [ubuntu-latest]\n        rust: [a, b]"
        ),
        2,
        "must be one axis or only `include` rows",
    ),
    Case(
        "a called workflow without workflow_call",
        lambda wf, _tc: _replace(wf / "e2e-codex.yml", "  workflow_call:\n", ""),
        2,
        "calls e2e-codex.yml, which declares no workflow_call trigger",
    ),
)


def run(case: Case, tmp: Path) -> subprocess.CompletedProcess[str]:
    workflows, toolchain = tmp / "workflows", tmp / "rust-toolchain.toml"
    shutil.copytree(ROOT / ".github" / "workflows", workflows)
    shutil.copy(ROOT / "rust-toolchain.toml", toolchain)
    case.edit(workflows, toolchain)
    args = [sys.executable, str(CHECKER), "--workflows", str(workflows), "--toolchain", str(toolchain)]
    return subprocess.run([*args, *case.extra_args], capture_output=True, text=True, check=False)


def main() -> None:
    for case in CASES:
        try:
            with tempfile.TemporaryDirectory() as tmp:
                out = run(case, Path(tmp))
        except OSError as exc:
            sys.exit(f"check-workflow-routing-test: cannot stage {case.label}: {exc}; check that $TMPDIR is writable")
        if out.returncode != case.exit_code or case.needle not in out.stdout + out.stderr:
            sys.exit(
                f"check-workflow-routing-test: {case.label}: expected exit {case.exit_code} naming {case.needle!r}, "
                f"got exit {out.returncode}:\n{out.stdout}{out.stderr}\n{FIX}"
            )
    print(f"check-workflow-routing-test: {len(CASES)} cases (green, drift, unmodellable input) behave")


if __name__ == "__main__":
    main()
