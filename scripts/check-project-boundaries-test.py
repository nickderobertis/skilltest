#!/usr/bin/env python3
# llmlint: ignore-file[new_code_lands_in_a_project] tests scripts/check-project-boundaries.py, a repo-level gate that belongs to no single Nx project; run workspace-wide from `just boundaries-check` (AGENTS.md: scripts/* are orchestrator-independent glue).
"""Test of scripts/check-project-boundaries.py, driven as a subprocess.

A gate nobody has watched fail is not known to work, so this runs the checker
over graphs in the shape `nx graph --file` writes: green on a clean graph, and
red — naming the offending edge or tag — on each kind of violation it exists to
catch, and on a file that is not a graph at all. Quiet on success, one line.
Python 3.11+ standard library only.
"""

from __future__ import annotations

import json
import subprocess
import sys
import tempfile
from dataclasses import dataclass, field, replace
from pathlib import Path

CHECKER = Path(__file__).resolve().parent / "check-project-boundaries.py"
FIX = "fix scripts/check-project-boundaries.py (or this test, if the rule changed on purpose)"


@dataclass(frozen=True)
class Fixture:
    """A project graph: each project's tags, and its edges as (source, target)."""

    tags: dict[str, tuple[str, ...]]
    edges: tuple[tuple[str, str], ...] = field(default=())

    def nx_json(self) -> dict:
        """The graph in the shape `nx graph --file` writes."""
        deps: dict[str, list[dict[str, str]]] = {name: [] for name in self.tags}
        for source, target in self.edges:
            deps.setdefault(source, []).append({"source": source, "target": target, "type": "implicit"})
        nodes = {name: {"type": "lib", "data": {"root": name, "tags": list(t)}} for name, t in self.tags.items()}
        return {"graph": {"nodes": nodes, "dependencies": deps}}

    def with_tags(self, name: str, *tags: str) -> Fixture:
        return replace(self, tags={**self.tags, name: tags})

    def with_edge(self, source: str, target: str) -> Fixture:
        return replace(self, edges=(*self.edges, (source, target)))


CLEAN = Fixture(
    tags={
        "core": ("type:lib", "lang:rust"),
        "cli": ("type:app", "lang:rust"),
        "contract": ("type:contract", "lang:json"),
        "sdk": ("type:sdk", "lang:python"),
        "plugin": ("type:plugin", "lang:python"),
        "cli-e2e": ("type:e2e", "lang:rust"),
        "live": ("type:live", "lang:rust"),
    },
    edges=(
        ("cli", "core"),
        ("sdk", "cli"),
        ("sdk", "contract"),
        ("sdk", "npm:left-pad"),  # an external node, not a project here: skipped
        ("plugin", "sdk"),
        ("cli-e2e", "cli"),
        ("live", "cli"),
    ),
)


def run(raw: object) -> subprocess.CompletedProcess[str]:
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "graph.json"
        path.write_text(json.dumps(raw))
        return subprocess.run([sys.executable, str(CHECKER), str(path)], capture_output=True, text=True, check=False)


def expect(label: str, raw: object, code: int, needle: str) -> None:
    out = run(raw)
    if out.returncode != code or needle not in out.stdout + out.stderr:
        sys.exit(
            f"check-project-boundaries-test: {label}: expected exit {code} naming {needle!r}, "
            f"got exit {out.returncode}:\n{out.stdout}{out.stderr}\n{FIX}"
        )


def main() -> None:
    expect("a clean graph", CLEAN.nx_json(), 0, "7 projects within their boundaries")
    red = [
        ("contract -> app", CLEAN.with_edge("contract", "cli"), "contract (type:contract) -> cli (type:app)"),
        ("lib -> live", CLEAN.with_edge("core", "live"), "core (type:lib) -> live (type:live)"),
        ("app -> e2e", CLEAN.with_edge("cli", "cli-e2e"), "cli (type:app) -> cli-e2e (type:e2e)"),
        ("missing lang tag", CLEAN.with_tags("core", "type:lib"), "core: needs exactly one `lang:*` tag"),
        ("two type tags", CLEAN.with_tags("core", "type:lib", "type:app", "lang:rust"), "has 2"),
        ("unknown type", CLEAN.with_tags("core", "type:utils", "lang:rust"), "core: unknown tag `type:utils`"),
    ]
    for label, fixture, needle in red:
        expect(label, fixture.nx_json(), 1, needle)
    expect("not a graph", {"nodes": {}}, 2, "top level must be an object with a `graph` object")
    print(f"check-project-boundaries-test: clean graph passes; {len(red)} violation kinds and a malformed graph fail")


if __name__ == "__main__":
    main()
