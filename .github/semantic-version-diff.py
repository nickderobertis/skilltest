"""Parse release TOML and normalize only this workspace's version fields.

Exit codes: 0 normalized; 1 the manifest is not an admitted release manifest;
2 invalid invocation.
"""

import json
import re
import sys
import tomllib

VERSION = re.compile(r"^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$")
# This workspace's own packages, by exact name and location. Anything else is
# dependency content; a member added later is refused (the Mac lane runs) until
# it is listed here, never silently admitted.
CORE_PATH = "crates/skilltest-core"
CARGO_MEMBERS = frozenset(
    {
        "skilltest-cli",
        "skilltest-cli-e2e",
        "skilltest-core",
        "skilltest-live-claude",
        "skilltest-live-judge-api",
    }
)
UV_MEMBERS = {"skilltest-sdk": "sdks/python", "skilltest-pytest": "plugins/pytest"}


def table(value: object, where: str) -> dict:
    if not isinstance(value, dict):
        raise ValueError(f"{where} must be a table")
    return value


def records(value: object, where: str) -> list[dict]:
    if not isinstance(value, list):
        raise ValueError(f"{where} must be an array")
    return [table(item, where) for item in value]


def erase(obj: dict) -> None:
    version = obj.get("version")
    if not isinstance(version, str) or not VERSION.fullmatch(version):
        raise ValueError("invalid or missing version")
    obj["version"] = "<version>"


def main() -> None:
    if len(sys.argv) != 2:
        print(
            "semantic-version-diff: usage: semantic-version-diff.py <admitted TOML path> < manifest",
            file=sys.stderr,
        )
        sys.exit(2)
    path = sys.argv[1]
    doc = table(tomllib.loads(sys.stdin.read()), "document")
    match path:
        case "Cargo.toml":
            workspace = table(doc.get("workspace"), "workspace")
            erase(table(workspace.get("package"), "workspace.package"))
            deps = table(workspace.get("dependencies"), "workspace.dependencies")
            core = table(deps.get("skilltest-core"), "skilltest-core dependency")
            # Only the workspace's own crate path is a version field; a registry
            # crate or another path sharing the name is dependency content.
            if core.get("path") != CORE_PATH:
                raise ValueError("skilltest-core dependency is not the workspace crate")
            erase(core)
        case "Cargo.lock":
            for package in records(doc.get("package"), "package"):
                name = package.get("name")
                if not isinstance(name, str):
                    raise ValueError("package name must be a string")
                if name in CARGO_MEMBERS and "source" not in package:
                    erase(package)
        case "sdks/python/pyproject.toml" | "plugins/pytest/pyproject.toml":
            project = table(doc.get("project"), "project")
            erase(project)
            deps = project.get("dependencies", [])
            if not isinstance(deps, list) or not all(isinstance(d, str) for d in deps):
                raise ValueError("project dependencies must be strings")
            sources = table(
                table(doc.get("tool", {}), "tool").get("uv", {}), "tool.uv"
            ).get("sources", {})
            # The SDK pin is a version field only while uv resolves it from the
            # workspace; otherwise it names a registry package.
            if table(sources, "tool.uv.sources").get("skilltest-sdk") == {
                "workspace": True
            }:
                project["dependencies"] = [
                    re.sub(
                        r"^skilltest-sdk==\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$",
                        "skilltest-sdk==<version>",
                        d,
                    )
                    for d in deps
                ]
        case "uv.lock":
            packages = records(doc.get("package"), "package")

            # A member is editable from its own directory; a registry or
            # elsewhere-editable package sharing its name is dependency content.
            def member(package: dict) -> bool:
                source = table(package.get("source", {}), "package source")
                name = package.get("name")
                return isinstance(name, str) and source == {
                    "editable": UV_MEMBERS.get(name)
                }

            local = {package.get("name") for package in packages if member(package)}
            for package in packages:
                if not member(package):
                    continue
                erase(package)
                metadata = table(package.get("metadata", {}), "package metadata")
                for dep in records(metadata.get("requires-dist", []), "requires-dist"):
                    specifier = dep.get("specifier", "")
                    if not isinstance(specifier, str):
                        raise ValueError("dependency specifier must be a string")
                    if (
                        dep.get("name") == "skilltest-sdk"
                        and "skilltest-sdk" in local
                        and specifier.startswith("==")
                    ):
                        if not VERSION.fullmatch(specifier[2:]):
                            raise ValueError("invalid SDK pin")
                        dep["specifier"] = "==<version>"
        case _:
            raise ValueError("unsupported TOML path")
    print(json.dumps(doc, sort_keys=True))


if __name__ == "__main__":
    try:
        main()
    except (ValueError, TypeError, KeyError) as error:
        sys.exit(
            f"semantic-version-diff: {error}; restore a valid admitted release manifest"
        )
