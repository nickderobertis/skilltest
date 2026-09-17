"""The published wheel is a typed package (PEP 561).

Consumers type-check against the *wheel*, not this tree, so these tests build
it the way the release does (``uv build --wheel``, as ``scripts/build-python-
dist.sh`` / ``build-python-wheel.sh`` do for ``publish.yml``) and read the zip:
``skilltest_sdk/py.typed`` must be a member and ``METADATA`` must carry the
``Typing :: Typed`` classifier. Without the marker a consumer's mypy reports
``import-untyped`` and every imported name degrades to ``Any``.

The pure ``py3-none-any`` wheel is the one built here (an empty ``_bin``); the
platform wheels are that wheel retagged, so its marker is every wheel's marker.

``skilltest_sdk/`` is partly generated (``scripts/gen-contract.sh`` owns the
four ``_*.py`` models), so a second journey runs the real generator and shows
it leaves the marker — and everything else it does not own — untouched.
"""

from __future__ import annotations

import hashlib
import shutil
import subprocess
import zipfile
from pathlib import Path

PROJECT = Path(__file__).resolve().parents[1]
REPO_ROOT = PROJECT.parents[1]
PACKAGE = "skilltest_sdk"
MARKER = PROJECT / PACKAGE / "py.typed"
GEN_CONTRACT = REPO_ROOT / "scripts" / "gen-contract.sh"
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
        ignore=shutil.ignore_patterns("py.typed", "_bin", "__pycache__"),
    )
    manifest = stripped / "pyproject.toml"
    manifest.write_text(manifest.read_text().replace('classifiers = ["Typing :: Typed"]\n', ""))

    wheel = build_wheel(stripped, tmp_path / "dist")

    assert f"{PACKAGE}/py.typed" not in wheel_members(wheel)
    assert CLASSIFIER not in wheel_metadata(wheel).splitlines()


def _package_digest() -> dict[str, str]:
    return {
        str(p.relative_to(PROJECT)): hashlib.sha256(p.read_bytes()).hexdigest()
        for p in sorted((PROJECT / PACKAGE).rglob("*"))
        if p.is_file() and "__pycache__" not in p.parts
    }


def test_contract_regeneration_keeps_marker() -> None:
    """Running the real generator (write mode) rewrites only the four models it
    owns; the marker is outside that set and survives with the rest of the
    package byte-for-byte. Needs ``cargo``/``uv``/``pnpm`` like ``just
    bootstrap`` does; a missing toolchain fails loudly rather than skipping."""
    assert MARKER.is_file(), "py.typed must exist before regeneration"
    before = _package_digest()

    subprocess.run(
        ["bash", str(GEN_CONTRACT)],
        cwd=REPO_ROOT,
        check=True,
        capture_output=True,
        text=True,
    )

    assert MARKER.is_file() and MARKER.read_bytes() == b""
    assert _package_digest() == before
