#!/usr/bin/env python3
"""Derive recursive partial objects from Config; array elements retain full types."""
import copy
import json
import sys
from pathlib import Path

def fail(error: Exception) -> None:
    print(f"could not derive ConfigOverrides schema: {error}; run just gen-contract with readable Config schema and writable staging paths", file=sys.stderr)
    raise SystemExit(1)


try:
    source = json.loads(Path(sys.argv[1]).read_text())
except (IndexError, OSError, ValueError) as error:
    fail(error)
result = copy.deepcopy(source)
definitions = result.setdefault("definitions", {})
originals = copy.deepcopy(definitions)


def partial(node):
    node = copy.deepcopy(node)
    node.pop("default", None)
    if "$ref" in node:
        name = node["$ref"].rsplit("/", 1)[-1]
        override = f"{name}Overrides"
        if override not in definitions:
            definitions[override] = {}
            try:
                definitions[override] = partial(originals[name])
            except KeyError as error:
                fail(error)
        node["$ref"] = f"#/definitions/{override}"
    if "properties" in node:
        node.pop("required", None)
        node["properties"] = {key: partial(value) for key, value in node["properties"].items()}
    for key in ("oneOf", "anyOf", "allOf"):
        if key in node:
            node[key] = [partial(branch) for branch in node[key]]
    # No traversal into items: an array is an atomic complete replacement.
    return node


root = partial({key: value for key, value in source.items() if key != "definitions"})
root["title"] = "ConfigOverrides"
root["definitions"] = definitions
try:
    Path(sys.argv[2]).write_text(json.dumps(root, indent=2) + "\n")
except (IndexError, OSError) as error:
    fail(error)
