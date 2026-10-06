#!/usr/bin/env python3
# llmlint: ignore-file[new_code_lands_in_a_project] repo-level gate over every project's tags and edges, which belongs to no single Nx project; run workspace-wide from `just check` like scripts/check-release-targets.sh (AGENTS.md: scripts/* are orchestrator-independent glue).
"""Module-boundary gate: every project's tags, and every edge, against the rules.

Nx's own module-boundary rule is an ESLint rule over JS imports, so it cannot see
a Cargo, uv or shell edge. This reads the project graph Nx computes
(`nx graph --file=<path>`, passed in as the one argument) and enforces the same
idea for every language here:

* every project carries exactly one known `type:*` tag and one known `lang:*` tag;
* every dependency edge goes from a type to a type that type may depend on
  (`ALLOWED` below). Nothing may depend on an `e2e` or `live` project, and a
  `contract` project depends only on other contracts — so a schema edit selects
  its consumers, never the reverse, and an expensive suite stays behind an edge
  no library can draw back.

Quiet on success, one line. On failure it names each violation and the fix.
Python 3.11+ standard library only.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

# Which project types each type may depend on. The rationale per row lives in
# AGENTS.md ("Project graph"); change the two together.
ALLOWED: dict[str, frozenset[str]] = {
    "contract": frozenset({"contract"}),
    "lib": frozenset({"lib", "contract"}),
    "app": frozenset({"lib", "contract"}),
    "sdk": frozenset({"app", "contract", "carrier"}),
    "plugin": frozenset({"sdk"}),
    "carrier": frozenset(),
    "e2e": frozenset({"app", "lib", "contract"}),
    "live": frozenset({"app", "lib", "contract"}),
}
LANGS = frozenset({"rust", "python", "typescript", "bash", "json"})


def _one_tag(name: str, tags: list[str], prefix: str, known: frozenset[str] | dict) -> tuple[str | None, list[str]]:
    values = [t.removeprefix(prefix) for t in tags if t.startswith(prefix)]
    if len(values) != 1:
        return None, [
            f"{name}: needs exactly one `{prefix}*` tag, has {len(values)} ({', '.join(values) or 'none'}); "
            f"set `tags` in its project.json to one of: {', '.join(prefix + k for k in sorted(known))}"
        ]
    if values[0] not in known:
        return None, [
            f"{name}: unknown tag `{prefix}{values[0]}`; use one of: {', '.join(prefix + k for k in sorted(known))}"
            " (or add it to scripts/check-project-boundaries.py and AGENTS.md together)"
        ]
    return values[0], []


def violations(graph: dict) -> list[str]:
    """Every rule the graph breaks, as one actionable line each."""
    nodes = graph["nodes"]
    problems: list[str] = []
    types: dict[str, str | None] = {}
    for name in sorted(nodes):
        tags = nodes[name].get("data", {}).get("tags", []) or []
        kind, errs = _one_tag(name, tags, "type:", ALLOWED)
        _, lang_errs = _one_tag(name, tags, "lang:", LANGS)
        types[name] = kind
        problems += errs + lang_errs
    for source in sorted(graph.get("dependencies", {})):
        for edge in graph["dependencies"][source]:
            target = edge["target"]
            if target not in nodes:
                continue  # an npm/external node, not a project of this repo
            src_t, dst_t = types.get(source), types.get(target)
            if src_t is None or dst_t is None:
                continue  # already reported as a tag problem
            if dst_t not in ALLOWED[src_t]:
                problems.append(
                    f"{source} (type:{src_t}) -> {target} (type:{dst_t}): a type:{src_t} project may depend only on "
                    f"{', '.join('type:' + t for t in sorted(ALLOWED[src_t])) or 'nothing'}; "
                    "remove the edge (implicitDependencies / the manifest dependency) or move the code"
                )
    return problems


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print("usage: check-project-boundaries.py <nx-graph.json>  (from `nx graph --file=...`)", file=sys.stderr)
        return 2
    try:
        graph = json.loads(Path(argv[1]).read_text())["graph"]
    except (OSError, ValueError, KeyError) as exc:
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
    print(f"check-project-boundaries: {len(graph['nodes'])} projects within their boundaries")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
