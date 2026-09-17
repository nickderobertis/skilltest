"""The published wheel is a typed package (PEP 561).

Consumers type-check against the *wheel*, not this tree, so this test builds it
the way the release does (``uv build --wheel``, as ``scripts/build-python-
dist.sh`` does for ``publish.yml``) and reads the zip: ``skilltest_pytest/
py.typed`` must be a member and ``METADATA`` must carry the ``Typing :: Typed``
classifier. Without the marker a consumer's mypy reports ``import-untyped`` and
every name re-exported from the SDK degrades to ``Any``.
"""

from __future__ import annotations

import shutil
import subprocess
import zipfile
from pathlib import Path

PROJECT = Path(__file__).resolve().parents[1]
PACKAGE = "skilltest_pytest"
CLASSIFIER = "Classifier: Typing :: Typed"


def build_wheel(project: Path, out_dir: Path) -> Path:
    # The same invocation the release scripts use, so the wheel inspected here
    # is built exactly as the one publish.yml uploads.
    subprocess.run(
        ["uv", "build", "--wheel", "--out-dir", str(out_dir)],
        cwd=project,
        check=True,
        capture_output=True,
        text=True,
    )
    wheels = sorted(out_dir.glob("*.whl"))
    assert len(wheels) == 1, f"expected exactly one wheel in {out_dir}, got {wheels}"
    return wheels[0]


def wheel_metadata(wheel: Path) -> str:
    with zipfile.ZipFile(wheel) as zf:
        (metadata,) = [n for n in zf.namelist() if n.endswith(".dist-info/METADATA")]
        return zf.read(metadata).decode()


def wheel_members(wheel: Path) -> set[str]:
    with zipfile.ZipFile(wheel) as zf:
        return set(zf.namelist())


def test_wheel_ships_py_typed_marker_and_classifier(tmp_path: Path) -> None:
    wheel = build_wheel(PROJECT, tmp_path)

    assert wheel.name.endswith("-py3-none-any.whl"), wheel.name
    members = wheel_members(wheel)
    assert f"{PACKAGE}/py.typed" in members, sorted(members)
    with zipfile.ZipFile(wheel) as zf:
        assert zf.read(f"{PACKAGE}/py.typed") == b"", "py.typed must be empty"
    assert CLASSIFIER in wheel_metadata(wheel).splitlines()


def test_stripped_wheel_lacks_marker_and_classifier(tmp_path: Path) -> None:
    """The assertions above discriminate: a copy of the project stripped of the
    marker and the classifier builds a wheel that carries neither."""
    stripped = tmp_path / "stripped"
    stripped.mkdir()
    for name in ("pyproject.toml", "README.md"):
        shutil.copy(PROJECT / name, stripped / name)
    shutil.copytree(
        PROJECT / PACKAGE,
        stripped / PACKAGE,
        ignore=shutil.ignore_patterns("py.typed", "__pycache__"),
    )
    manifest = stripped / "pyproject.toml"
    manifest.write_text(manifest.read_text().replace('classifiers = ["Typing :: Typed"]\n', ""))

    wheel = build_wheel(stripped, tmp_path / "dist")

    assert f"{PACKAGE}/py.typed" not in wheel_members(wheel)
    assert CLASSIFIER not in wheel_metadata(wheel).splitlines()
