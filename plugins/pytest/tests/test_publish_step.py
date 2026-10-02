"""`publish.yml` publishes exactly the dists its build step writes.

The plugin is a member of the repo's uv workspace, and a bare `uv build` inside
a workspace writes to the workspace root's `dist/`, not the member's. The
release step then published `dist/*` from `plugins/pytest` and found nothing,
so PyPI kept serving an old plugin while the SDK moved on (skilltest#59).

This test replays the workflow's own skilltest-pytest step, read from
`publish.yml` so it cannot drift from what CI runs, over a scratch copy of the
workspace, with `uv publish` swapped for a listing of what it would upload.
The listing must name exactly the wheel and sdist the build wrote.
"""

from __future__ import annotations

import re
import shutil
import subprocess
from pathlib import Path

PROJECT = Path(__file__).resolve().parents[1]
REPO_ROOT = PROJECT.parents[1]
WORKFLOW = REPO_ROOT / ".github" / "workflows" / "publish.yml"
MEMBERS = ("sdks/python", "plugins/pytest")


def publish_step() -> str:
    """The subshell that builds and publishes skilltest-pytest."""
    (step,) = re.findall(r"\( cd plugins/pytest && .* \)", WORKFLOW.read_text())
    return step


def copy_workspace(scratch: Path) -> None:
    """The workspace root plus both members' tracked files — what `uv build`
    reads — so the build cannot write into this tree."""
    for name in ("pyproject.toml", "uv.lock"):
        shutil.copy(REPO_ROOT / name, scratch / name)
    listed = subprocess.run(
        ["git", "ls-files", "-z", *MEMBERS], cwd=REPO_ROOT, check=True, capture_output=True
    )
    for name in filter(None, listed.stdout.decode().split("\0")):
        destination = scratch / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy(REPO_ROOT / name, destination)


def built_dists(root: Path) -> set[Path]:
    return {p for p in root.rglob("*") if p.name.endswith((".whl", ".tar.gz"))}


# Not a marker-deselected tier: this runs unconditionally in the project's `test-e2e`
# target, which is its whole test tier by design; a pure build reaches no external service.
# llmlint: ignore[test_tiers_split_by_project_not_by_marker] unconditional in the project's own tier
def test_publish_step_uploads_exactly_what_its_build_wrote(tmp_path: Path) -> None:
    step = publish_step()
    assert "uv publish " in step, step
    dry_run = step.replace("uv publish ", "printf '%s\\n' ")
    copy_workspace(tmp_path)

    ran = subprocess.run(
        ["bash", "-euo", "pipefail", "-c", dry_run],
        cwd=tmp_path,
        capture_output=True,
        text=True,
    )
    assert ran.returncode == 0, ran.stderr

    published = {(tmp_path / "plugins/pytest" / line).resolve() for line in ran.stdout.split()}
    built = {p.resolve() for p in built_dists(tmp_path)}
    assert len(built) == 2, f"expected one wheel and one sdist, got {sorted(built)}"
    assert published == built, f"publishes {sorted(published)} but built {sorted(built)}"
