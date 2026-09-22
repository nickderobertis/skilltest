"""The repo's two Python packages are one uv workspace with one lockfile.

`skilltest-pytest` depends on `skilltest-sdk`, so resolving them separately let
the two drift: each had its own `uv.lock`, and a change to the SDK's
requirements was only seen by the plugin after a second, independent lock
update. The root `pyproject.toml` (`[tool.uv.workspace]`) makes them members of
one workspace resolved into one `uv.lock`, and the plugin's `[tool.uv.sources]`
entry resolves the SDK to that member in development.

These tests drive the real `uv` the way `just bootstrap` and
`scripts/set-version.sh` do, from the tree as it stands. The failure path is
drift itself: adding a requirement to *either* member must leave the single root
lock out of date — which is only true if that one lock really covers both.
"""

from __future__ import annotations

import shutil
import subprocess
import tomllib
from pathlib import Path

import pytest

PROJECT = Path(__file__).resolve().parents[1]
REPO_ROOT = PROJECT.parents[1]
MEMBERS = ("sdks/python", "plugins/pytest")


def uv_lock_check(project_root: Path) -> subprocess.CompletedProcess[str]:
    """`uv lock --check`: does the lockfile match the members' requirements?"""
    return subprocess.run(
        ["uv", "lock", "--check"],
        cwd=project_root,
        capture_output=True,
        text=True,
    )


def test_one_lockfile_covers_both_python_packages() -> None:
    assert (REPO_ROOT / "uv.lock").is_file(), "the workspace lock must live at the repo root"
    for member in MEMBERS:
        stale = REPO_ROOT / member / "uv.lock"
        assert not stale.exists(), f"{member} must not carry its own lock: the workspace has one"

    workspace = tomllib.loads((REPO_ROOT / "pyproject.toml").read_text())["tool"]["uv"]["workspace"]
    assert sorted(workspace["members"]) == sorted(MEMBERS)

    locked = tomllib.loads((REPO_ROOT / "uv.lock").read_text())
    assert sorted(locked["manifest"]["members"]) == ["skilltest-pytest", "skilltest-sdk"]

    checked = uv_lock_check(REPO_ROOT)
    assert checked.returncode == 0, checked.stderr


def test_plugin_resolves_the_sdk_to_the_workspace_member() -> None:
    """A dev run of the plugin imports the SDK *from this tree*, not a release:
    editing `sdks/python` is immediately what `plugins/pytest` runs against."""
    run = subprocess.run(
        ["uv", "run", "--directory", str(PROJECT), "python", "-c", IMPORT_SDK],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
    )
    assert run.returncode == 0, run.stderr

    resolved = Path(run.stdout.strip())
    assert resolved.is_relative_to(REPO_ROOT / "sdks" / "python"), resolved


IMPORT_SDK = "import skilltest_sdk; print(skilltest_sdk.__file__)"


@pytest.mark.parametrize("member", MEMBERS)
def test_a_requirement_added_to_either_member_makes_the_one_lock_stale(
    member: str, tmp_path: Path
) -> None:
    """The discriminating case: in a scratch copy of the workspace the lock is
    current, and adding a dependency to either member's manifest makes `uv lock
    --check` report it stale. Per-member locks could not see the other member."""
    scratch = tmp_path / "workspace"
    scratch.mkdir()
    shutil.copy(REPO_ROOT / "pyproject.toml", scratch / "pyproject.toml")
    shutil.copy(REPO_ROOT / "uv.lock", scratch / "uv.lock")
    for name in MEMBERS:
        (scratch / name).mkdir(parents=True)
        shutil.copy(REPO_ROOT / name / "pyproject.toml", scratch / name / "pyproject.toml")

    assert uv_lock_check(scratch).returncode == 0, "the copied workspace starts in sync"

    manifest = scratch / member / "pyproject.toml"
    manifest.write_text(
        manifest.read_text().replace("dependencies = [", 'dependencies = [\n    "iniconfig",', 1)
    )

    drifted = uv_lock_check(scratch)
    assert drifted.returncode != 0, drifted.stdout


def test_set_version_moves_both_members_and_the_one_lock(tmp_path: Path) -> None:
    """The release path keeps the single lock current.

    `scripts/set-version.sh` is what semantic-release runs to write the lockstep
    version, and one `uv lock` at the root is now all it runs for Python. Drive
    the real script over a copy of the tracked tree and require the bump to land
    in both members *and* in the one lock — `uv lock --check` afterwards is the
    assertion that the refresh covered both, not just the member locked last.
    """
    scratch = tmp_path / "repo"
    tracked = subprocess.run(
        ["git", "ls-files", "-z"],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        check=True,
    ).stdout.split("\0")
    for name in filter(None, tracked):
        destination = scratch / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy(REPO_ROOT / name, destination)

    bumped = subprocess.run(
        ["bash", "scripts/set-version.sh", RELEASE_VERSION],
        cwd=scratch,
        capture_output=True,
        text=True,
    )
    assert bumped.returncode == 0, bumped.stderr

    for member in MEMBERS:
        manifest = tomllib.loads((scratch / member / "pyproject.toml").read_text())
        assert manifest["project"]["version"] == RELEASE_VERSION, member
        assert not (scratch / member / "uv.lock").exists(), member

    plugin = tomllib.loads((scratch / "plugins/pytest/pyproject.toml").read_text())
    assert f"skilltest-sdk=={RELEASE_VERSION}" in plugin["project"]["dependencies"]

    locked = tomllib.loads((scratch / "uv.lock").read_text())
    members = {
        p["name"]: p["version"] for p in locked["package"] if p["name"].startswith("skilltest-")
    }
    assert members == {"skilltest-sdk": RELEASE_VERSION, "skilltest-pytest": RELEASE_VERSION}

    checked = uv_lock_check(scratch)
    assert checked.returncode == 0, checked.stderr


# A version no release will ever cut, so a leak out of the scratch copy is obvious.
RELEASE_VERSION = "9.9.9"
