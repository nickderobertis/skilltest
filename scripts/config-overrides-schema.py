#!/usr/bin/env python3
"""Derive recursive partial objects from Config; array elements retain full types."""
import copy
import json
import sys
from pathlib import Path

source = json.loads(Path(sys.argv[1]).read_text())
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
            definitions[override] = partial(originals[name])
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
Path(sys.argv[2]).write_text(json.dumps(root, indent=2) + "\n")
