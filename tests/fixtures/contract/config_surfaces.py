"""Schema-derived surface examples. Arrays are leaves; objects/variants recurse."""
from __future__ import annotations

from typing import Any


def examples(schema: dict[str, Any]) -> list[tuple[str, Any, dict[str, Any]]]:
    def dereference(node: dict[str, Any]) -> dict[str, Any]:
        if "$ref" in node:
            return schema["definitions"][node["$ref"].rsplit("/", 1)[1]]
        return node

    def variants(node: dict[str, Any]) -> list[dict[str, Any]]:
        node = dereference(node)
        for branch in ("oneOf", "anyOf", "allOf"):
            if branch in node:
                return [part for child in node[branch] for part in variants(child)
                        if part.get("type") != "null"]
        return [node]

    def sample(node: dict[str, Any]) -> Any:
        node = variants(node)[0]
        if "const" in node:
            return node["const"]
        if "enum" in node:
            return node["enum"][0]
        kind = node.get("type")
        if isinstance(kind, list):
            kind = next(value for value in kind if value != "null")
        if kind == "object":
            return {key: sample(value) for key, value in node.get("properties", {}).items()}
        if "default" in node and node["default"] is not None:
            return node["default"]
        if kind == "array":
            return []
        if kind == "boolean":
            return False
        if kind in ("integer", "number"):
            return max(1, node.get("minimum", 0))
        return "surface value with spaces"

    output = []

    def walk(node: dict[str, Any], prefix: str, context: dict[str, Any]) -> None:
        for variant in variants(node):
            if "properties" in variant:
                parent = sample(variant)
                for key, child in variant["properties"].items():
                    path = f"{prefix}.{key}" if prefix else key
                    walk(child, path, {**context, prefix: parent} if prefix else context)
            else:
                output.append((prefix, sample(variant), context))

    walk(schema, "", {})
    return output

if __name__ == "__main__":
    import json
    import sys
    from pathlib import Path

    print(json.dumps(examples(json.loads(Path(sys.argv[1]).read_text()))))
