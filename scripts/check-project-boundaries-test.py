#!/usr/bin/env python3
# llmlint: ignore-file[new_code_lands_in_a_project] tests scripts/check-project-boundaries.py, a repo-level gate that belongs to no single Nx project; run workspace-wide from `just boundaries-check` (AGENTS.md: scripts/* are orchestrator-independent glue).
"""Test of scripts/check-project-boundaries.py, driven as a subprocess.

A gate nobody has watched fail is not known to work, so this runs the checker
over graphs in the shape `nx graph --file` writes: green on a clean graph; red
(exit 1), naming the offending edge or tag, on each kind of violation it exists
to catch; and refused (exit 2) on every input that is not such a graph, and on
a bad invocation. Quiet on success, one line. Python 3.11+ standard library only.
"""

from __future__ import annotations

import json
import subprocess
import sys
import tempfile
from dataclasses import dataclass, field, replace
from pathlib import Path
from typing import NamedTuple, NewType

CHECKER = Path(__file__).resolve().parent / "check-project-boundaries.py"
FIX = "fix scripts/check-project-boundaries.py (or this test, if the rule changed on purpose)"

ProjectId = NewType("ProjectId", str)
Nodes = dict[str, dict[str, object]]
Deps = dict[str, list[dict[str, str]]]


def nx_json(nodes: Nodes, deps: Deps) -> dict[str, object]:
    return {"graph": {"nodes": nodes, "dependencies": deps}}


class Edge(NamedTuple):
    source: ProjectId
    target: ProjectId


@dataclass(frozen=True)
class Fixture:
    """A project graph: each project's tags, and its edges."""

    tags: dict[ProjectId, tuple[str, ...]]
    edges: tuple[Edge, ...] = field(default=())

    def parts(self) -> tuple[Nodes, Deps]:
        """The graph's `nodes` and `dependencies`, in the shape `nx graph --file` writes."""
        deps: Deps = {name: [] for name in self.tags}
        for edge in self.edges:
            deps.setdefault(edge.source, []).append({"source": edge.source, "target": edge.target, "type": "implicit"})
        nodes: Nodes = {name: {"type": "lib", "data": {"root": name, "tags": list(t)}} for name, t in self.tags.items()}
        return nodes, deps

    def nx_json(self) -> dict[str, object]:
        return nx_json(*self.parts())

    def with_tags(self, name: str, *tags: str) -> Fixture:
        return replace(self, tags={**self.tags, ProjectId(name): tags})

    def with_edge(self, source: str, target: str) -> Fixture:
        return replace(self, edges=(*self.edges, Edge(ProjectId(source), ProjectId(target))))


def _p(name: str) -> ProjectId:
    return ProjectId(name)


CLEAN = Fixture(
    tags={
        _p("core"): ("type:lib", "lang:rust"),
        _p("cli"): ("type:app", "lang:rust"),
        _p("contract"): ("type:contract", "lang:json"),
        _p("sdk"): ("type:sdk", "lang:python"),
        _p("plugin"): ("type:plugin", "lang:python"),
        _p("cli-e2e"): ("type:e2e", "lang:rust"),
        _p("live"): ("type:live", "lang:rust"),
    },
    edges=(
        Edge(_p("cli"), _p("core")),
        Edge(_p("sdk"), _p("cli")),
        Edge(_p("sdk"), _p("contract")),
        Edge(_p("sdk"), _p("npm:left-pad")),  # an external node, not a project here: skipped
        Edge(_p("plugin"), _p("sdk")),
        Edge(_p("cli-e2e"), _p("cli")),
        Edge(_p("live"), _p("cli")),
    ),
)


class Case(NamedTuple):
    label: str
    graph_json: str | None  # None: pass no file argument at all
    exit_code: int
    needle: str


def run(case: Case, tmp: Path) -> subprocess.CompletedProcess[str]:
    args = [sys.executable, str(CHECKER)]
    if case.graph_json is not None:
        path = tmp / "graph.json"
        path.write_text(case.graph_json)
        args.append(str(path))
    return subprocess.run(args, capture_output=True, text=True, check=False)


def graph_case(label: str, raw: object, exit_code: int, needle: str) -> Case:
    return Case(label, json.dumps(raw), exit_code, needle)


def cases() -> list[Case]:
    nodes, deps = CLEAN.parts()
    nodes["core"]["data"] = None
    broken_node = nx_json(nodes, deps)
    nodes, deps = CLEAN.parts()
    deps["ghost"] = []
    stray_source = nx_json(nodes, deps)
    nodes, deps = CLEAN.parts()
    deps["cli"] = [{"source": "cli"}]
    no_target = nx_json(nodes, deps)
    return [
        graph_case("a clean graph", CLEAN.nx_json(), 0, "7 projects within their boundaries"),
        graph_case(
            "contract -> app", CLEAN.with_edge("contract", "cli").nx_json(), 1, "contract (type:contract) -> cli"
        ),
        graph_case("lib -> live", CLEAN.with_edge("core", "live").nx_json(), 1, "core (type:lib) -> live (type:live)"),
        graph_case("app -> e2e", CLEAN.with_edge("cli", "cli-e2e").nx_json(), 1, "cli (type:app) -> cli-e2e"),
        graph_case("missing lang tag", CLEAN.with_tags("core", "type:lib").nx_json(), 1, "exactly one `lang:*` tag"),
        graph_case("two type tags", CLEAN.with_tags("core", "type:lib", "type:app", "lang:rust").nx_json(), 1, "has 2"),
        graph_case("unknown type", CLEAN.with_tags("core", "type:utils", "lang:rust").nx_json(), 1, "`type:utils`"),
        graph_case("not a graph", {"nodes": {}}, 2, "top level must be an object with a `graph.nodes` object"),
        graph_case("a node without data", broken_node, 2, "node `core` needs a `data` object"),
        graph_case("edges from an unknown node", stray_source, 2, "`ghost`, which is not a node"),
        graph_case("an edge without a target", no_target, 2, "an edge of `cli` has no string `target`"),
        Case("invalid JSON", "{not json", 2, "cannot read the nx graph"),
        Case("no graph argument", None, 2, "usage: check-project-boundaries.py <nx-graph.json>"),
    ]


def main() -> None:
    all_cases = cases()
    try:
        with tempfile.TemporaryDirectory() as tmp:
            for case in all_cases:
                out = run(case, Path(tmp))
                if out.returncode != case.exit_code or case.needle not in out.stdout + out.stderr:
                    sys.exit(
                        f"check-project-boundaries-test: {case.label}: expected exit {case.exit_code} naming "
                        f"{case.needle!r}, got exit {out.returncode}:\n{out.stdout}{out.stderr}\n{FIX}"
                    )
    except OSError as exc:
        sys.exit(
            f"check-project-boundaries-test: cannot stage or run the checker: {exc}; "
            "check that $TMPDIR is writable and python3 can run scripts/check-project-boundaries.py"
        )
    print(f"check-project-boundaries-test: {len(all_cases)} cases (clean, violations, malformed input) behave")


if __name__ == "__main__":
    main()
