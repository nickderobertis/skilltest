"""Parse TOML release files; erase only this workspace's version fields."""

import json
import re
import sys
import tomllib

path = sys.argv[1]
doc = tomllib.loads(sys.stdin.read())
version = re.compile(r"^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$")


def erase(obj, key="version"):
    if key in obj:
        if not isinstance(obj[key], str) or not version.fullmatch(obj[key]):
            raise ValueError("invalid version")
        obj[key] = "<version>"


match path:
    case "Cargo.toml":
        erase(doc["workspace"]["package"])
        erase(doc["workspace"]["dependencies"]["skilltest-core"])
    case "Cargo.lock":
        for package in doc["package"]:
            if package["name"].startswith("skilltest") and "source" not in package:
                erase(package)
    case "sdks/python/pyproject.toml" | "plugins/pytest/pyproject.toml":
        erase(doc["project"])
        doc["project"]["dependencies"] = [
            re.sub(
                r"^skilltest-sdk==\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$",
                "skilltest-sdk==<version>",
                d,
            )
            for d in doc["project"].get("dependencies", [])
        ]
    case "uv.lock":
        for package in doc["package"]:
            if package["name"] in (
                "skilltest-sdk",
                "skilltest-pytest",
            ) and "editable" in package.get("source", {}):
                erase(package)
            for dep in package.get("metadata", {}).get("requires-dist", []):
                if dep["name"] == "skilltest-sdk" and dep.get(
                    "specifier", ""
                ).startswith("=="):
                    pin = dep["specifier"][2:]
                    if not version.fullmatch(pin):
                        raise ValueError("invalid SDK pin")
                    dep["specifier"] = "==<version>"
    case _:
        raise ValueError("unsupported TOML path")
print(json.dumps(doc, sort_keys=True))
