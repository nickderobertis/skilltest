# llmlint: ignore-file[new_code_lands_in_a_project] This shared fixture implementation belongs to skilltest-contract through its explicit tests/fixtures/contract named input; the dispatch requires shared config fixtures rather than a new unrelated project.
"""Schema-derived surface examples. Arrays are leaves; objects/variants recurse."""
from __future__ import annotations

from typing import Any, NamedTuple


# JSON Schema nodes mix keyword forms and arbitrary JSON specimens. Any is
# required for this schema walker; generated models and the real Config resolver
# validate every specimen before it is accepted as configuration.
class SurfaceExample(NamedTuple):
    path: str
    value: Any
    context: dict[str, Any]


def examples(schema: dict[str, Any]) -> list[SurfaceExample]:
    def dereference(node: dict[str, Any]) -> dict[str, Any]:
        if "$ref" in node:
            return schema["definitions"][node["$ref"].rsplit("/", 1)[1]]
        return node

    def variants(node: dict[str, Any]) -> list[dict[str, Any]]:
        node = dereference(node)
        match node:
            case {"oneOf": branches} | {"anyOf": branches} | {"allOf": branches}:
                return [part for child in branches for part in variants(child)
                        if part.get("type") != "null"]
            case _:
                return [node]

    def sample(node: dict[str, Any]) -> Any:
        node = variants(node)[0]
        match node:
            case {"const": constant}:
                return constant
            case {"enum": [first, *_]}:
                return first
        kind = node.get("type")
        if isinstance(kind, list):
            kind = next(value for value in kind if value != "null")
        if "default" in node and node["default"] is not None and kind != "object":
            return node["default"]
        match kind:
            case "object":
                return {key: sample(value) for key, value in node.get("properties", {}).items()}
            case "array":
                return []
            case "boolean":
                return False
            case "integer" | "number":
                return max(1, node.get("minimum", 0))
            case _:
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
                output.append(SurfaceExample(prefix, sample(variant), context))

    walk(schema, "", {})
    return output

if __name__ == "__main__":
    import json
    import sys
    from pathlib import Path

    print(json.dumps(examples(json.loads(Path(sys.argv[1]).read_text()))))
