#!/usr/bin/env python3
# llmlint: ignore-file[new_code_lands_in_a_project] tests scripts/check-project-boundaries.py, a repo-level gate that belongs to no single Nx project; run workspace-wide from `just boundaries-check` (AGENTS.md: scripts/* are orchestrator-independent glue).
"""Test of scripts/check-project-boundaries.py, driven as a subprocess.

A gate nobody has watched fail is not known to work, so this runs the checker
over graphs shaped like `nx graph --file` output: green on a clean graph, and
red — naming the offending edge or tag — on each kind of violation it exists to
catch. Quiet on success, one line. Python 3.11+ standard library only.
"""

from __future__ import annotations

import json
import subprocess
import sys
import tempfile
from pathlib import Path

CHECKER = Path(__file__).resolve().parent / "check-project-boundaries.py"


def node(tags: list[str]) -> dict:
    return {"type": "lib", "data": {"root": "x", "tags": tags}}


def clean_graph() -> dict:
    return {
        "nodes": {
            "core": node(["type:lib", "lang:rust"]),
            "cli": node(["type:app", "lang:rust"]),
            "contract": node(["type:contract", "lang:json"]),
            "sdk": node(["type:sdk", "lang:python"]),
            "plugin": node(["type:plugin", "lang:python"]),
            "cli-e2e": node(["type:e2e", "lang:rust"]),
            "live": node(["type:live", "lang:rust"]),
        },
        "dependencies": {
            "cli": [{"source": "cli", "target": "core", "type": "implicit"}],
            "sdk": [
                {"source": "sdk", "target": "cli", "type": "implicit"},
                {"source": "sdk", "target": "contract", "type": "implicit"},
                # An external (npm) node is not a project here and is skipped.
                {"source": "sdk", "target": "npm:left-pad", "type": "static"},
            ],
            "plugin": [{"source": "plugin", "target": "sdk", "type": "implicit"}],
            "cli-e2e": [{"source": "cli-e2e", "target": "cli", "type": "implicit"}],
            "live": [{"source": "live", "target": "cli", "type": "implicit"}],
        },
    }


def run(graph: dict) -> subprocess.CompletedProcess[str]:
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "graph.json"
        path.write_text(json.dumps({"graph": graph}))
        return subprocess.run([sys.executable, str(CHECKER), str(path)], capture_output=True, text=True, check=False)


def expect_red(label: str, graph: dict, needle: str) -> None:
    out = run(graph)
    if out.returncode != 1 or needle not in out.stderr:
        sys.exit(
            f"check-project-boundaries-test: {label}: expected exit 1 naming {needle!r}, "
            f"got exit {out.returncode}:\n{out.stdout}{out.stderr}"
        )


def main() -> None:
    out = run(clean_graph())
    if out.returncode != 0:
        sys.exit(f"check-project-boundaries-test: a clean graph must pass, got exit {out.returncode}:\n{out.stderr}")

    g = clean_graph()
    g["dependencies"]["contract"] = [{"source": "contract", "target": "cli", "type": "implicit"}]
    expect_red("contract -> app", g, "contract (type:contract) -> cli (type:app)")

    g = clean_graph()
    g["dependencies"]["core"] = [{"source": "core", "target": "live", "type": "implicit"}]
    expect_red("lib -> live", g, "core (type:lib) -> live (type:live)")

    g = clean_graph()
    g["dependencies"]["cli"].append({"source": "cli", "target": "cli-e2e", "type": "implicit"})
    expect_red("app -> e2e", g, "cli (type:app) -> cli-e2e (type:e2e)")

    g = clean_graph()
    g["nodes"]["core"] = node(["type:lib"])
    expect_red("missing lang tag", g, "core: needs exactly one `lang:*` tag")

    g = clean_graph()
    g["nodes"]["core"] = node(["type:lib", "type:app", "lang:rust"])
    expect_red("two type tags", g, "core: needs exactly one `type:*` tag, has 2")

    g = clean_graph()
    g["nodes"]["core"] = node(["type:utils", "lang:rust"])
    expect_red("unknown type", g, "core: unknown tag `type:utils`")

    print("check-project-boundaries-test: clean graph passes; 6 violation kinds each fail")


if __name__ == "__main__":
    main()
