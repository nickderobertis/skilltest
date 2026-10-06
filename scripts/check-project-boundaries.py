#!/usr/bin/env python3
# llmlint: ignore-file[new_code_lands_in_a_project] repo-level gate over every project's tags and edges, which belongs to no single Nx project; run workspace-wide from `just check` like scripts/check-release-targets.sh (AGENTS.md: scripts/* are orchestrator-independent glue).
"""Module-boundary gate: every project's tags, and every edge, against the rules.

Nx's own module-boundary rule is an ESLint rule over JS imports, so it cannot see
a Cargo, uv or shell edge. This reads the project graph Nx computes
(`nx graph --file=<path>`, passed in as the one argument) and enforces the same
idea for every language here:

* every project carries exactly one known `type:*` tag and one known `lang:*` tag;
* every dependency edge goes from a type to a type that type may depend on
  (`ALLOWED` below — the one source for these rules). Nothing may depend on an
  `e2e` or `live` project, and a `contract` project depends only on other
  contracts, so a schema edit selects its consumers, never the reverse, and an
  expensive suite stays behind an edge no library can draw back.

Quiet on success, one line. On failure it names each violation and the fix.
Python 3.11+ standard library only.
"""

from __future__ import annotations

import json
import sys
from dataclasses import dataclass
from enum import StrEnum
from pathlib import Path
from typing import NewType, TypeVar


class Kind(StrEnum):
    """A project's `type:*` tag."""

    CONTRACT = "contract"
    LIB = "lib"
    APP = "app"
    SDK = "sdk"
    PLUGIN = "plugin"
    CARRIER = "carrier"
    E2E = "e2e"
    LIVE = "live"


class Lang(StrEnum):
    """A project's `lang:*` tag."""

    RUST = "rust"
    PYTHON = "python"
    TYPESCRIPT = "typescript"
    BASH = "bash"
    JSON = "json"


# Which project kinds each kind may depend on.
ALLOWED: dict[Kind, frozenset[Kind]] = {
    Kind.CONTRACT: frozenset({Kind.CONTRACT}),
    Kind.LIB: frozenset({Kind.LIB, Kind.CONTRACT}),
    Kind.APP: frozenset({Kind.LIB, Kind.CONTRACT}),
    Kind.SDK: frozenset({Kind.APP, Kind.CONTRACT, Kind.CARRIER}),
    Kind.PLUGIN: frozenset({Kind.SDK}),
    Kind.CARRIER: frozenset(),
    Kind.E2E: frozenset({Kind.APP, Kind.LIB, Kind.CONTRACT}),
    Kind.LIVE: frozenset({Kind.APP, Kind.LIB, Kind.CONTRACT}),
}


class GraphError(ValueError):
    """The file is not the project graph `nx graph --file` writes."""


# A node name in the nx graph: a project of this repo, or an external
# (`npm:...`) node an edge may point at.
ProjectId = NewType("ProjectId", str)


@dataclass(frozen=True)
class Project:
    name: ProjectId
    tags: tuple[str, ...]


@dataclass(frozen=True)
class Edge:
    source: ProjectId
    target: ProjectId


@dataclass(frozen=True)
class Graph:
    projects: dict[ProjectId, Project]
    edges: tuple[Edge, ...]


def _project(name: str, node: object) -> Project:
    match node:
        case {"data": {"tags": list(tags)}} if all(isinstance(t, str) for t in tags):
            return Project(ProjectId(name), tuple(tags))
        case {"data": dict(data)} if "tags" not in data:
            return Project(ProjectId(name), ())
        case _:
            raise GraphError(f"node `{name}` needs a `data` object whose `tags` is a list of strings")


def parse_graph(raw: object) -> Graph:
    """Validate the `nx graph --file` JSON into a `Graph`, naming what is malformed."""
    match raw:
        case {"graph": {"nodes": dict(nodes), **rest}}:
            deps = rest.get("dependencies", {})
        case _:
            raise GraphError("top level must be an object with a `graph.nodes` object")
    if not isinstance(deps, dict):
        raise GraphError("`graph.dependencies` must be an object")
    projects = {ProjectId(name): _project(name, node) for name, node in nodes.items()}
    edges: list[Edge] = []
    for source, out in deps.items():
        if source not in projects:
            raise GraphError(f"`dependencies` has edges from `{source}`, which is not a node of the graph")
        if not isinstance(out, list):
            raise GraphError(f"`dependencies.{source}` must be a list")
        for edge in out:
            match edge:
                case {"target": str(target)} if target in projects or target.startswith("npm:"):
                    edges.append(Edge(ProjectId(source), ProjectId(target)))
                case {"target": str(target)}:
                    raise GraphError(f"an edge of `{source}` targets `{target}`, which is neither a node nor npm:*")
                case _:
                    raise GraphError(f"an edge of `{source}` has no string `target`")
    return Graph(projects, tuple(edges))


E = TypeVar("E", Kind, Lang)


def _one_tag(project: Project, prefix: str, known: type[E]) -> tuple[E | None, list[str]]:
    values = [t.removeprefix(prefix) for t in project.tags if t.startswith(prefix)]
    choices = ", ".join(prefix + k.value for k in known)
    if len(values) != 1:
        found = ", ".join(values) or "none"
        return None, [
            f"{project.name}: needs exactly one `{prefix}*` tag, has {len(values)} ({found}); "
            f"set `tags` in its project.json to one of: {choices}"
        ]
    try:
        return known(values[0]), []
    except ValueError:
        return None, [
            f"{project.name}: unknown tag `{prefix}{values[0]}`; use one of: {choices}"
            " (or add it to scripts/check-project-boundaries.py)"
        ]


def violations(graph: Graph) -> list[str]:
    """Every rule the graph breaks, as one actionable line each."""
    problems: list[str] = []
    kinds: dict[ProjectId, Kind | None] = {}
    for name in sorted(graph.projects):
        kind, errs = _one_tag(graph.projects[name], "type:", Kind)
        _, lang_errs = _one_tag(graph.projects[name], "lang:", Lang)
        kinds[name] = kind
        problems += errs + lang_errs
    for edge in sorted(graph.edges, key=lambda e: (e.source, e.target)):
        if edge.target not in graph.projects:
            continue  # an npm:* external node (parse_graph refuses anything else)
        src, dst = kinds.get(edge.source), kinds.get(edge.target)
        if src is None or dst is None:
            continue  # already reported as a tag problem
        if dst not in ALLOWED[src]:
            allowed = ", ".join(f"type:{k.value}" for k in sorted(ALLOWED[src])) or "nothing"
            problems.append(
                f"{edge.source} (type:{src.value}) -> {edge.target} (type:{dst.value}): a type:{src.value} "
                f"project may depend only on {allowed}; remove the edge (implicitDependencies / the "
                "manifest dependency) or move the code"
            )
    return problems


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print("usage: check-project-boundaries.py <nx-graph.json>  (from `nx graph --file=...`)", file=sys.stderr)
        return 2
    try:
        graph = parse_graph(json.loads(Path(argv[1]).read_text()))
    except (OSError, ValueError) as exc:
        print(
            f"check-project-boundaries: cannot read the nx graph from {argv[1]}: {exc}; "
            "regenerate it with `pnpm exec nx graph --file=<path>.json`",
            file=sys.stderr,
        )
        return 2
    problems = violations(graph)
    if problems:
        print(f"check-project-boundaries: {len(problems)} module-boundary violation(s):", file=sys.stderr)
        for p in problems:
            print(f"  - {p}", file=sys.stderr)
        return 1
    print(f"check-project-boundaries: {len(graph.projects)} projects within their boundaries")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
