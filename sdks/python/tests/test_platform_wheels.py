"""The release's wheel scripts build one correctly tagged wheel per platform.

``publish.yml``'s PyPI job runs ``scripts/build-python-dist.sh`` over the
per-target binaries, which calls ``scripts/build-python-wheel.sh`` once per
target, then builds the pure wheel and sdist. These tests run those scripts
as the job does — over a scratch copy of the uv workspace, so nothing is
written into this tree — with a stand-in file per target, and read the zips:
each platform wheel must carry its platform tag and exactly its bundled binary
(``skilltest.exe`` on Windows, the name the runner resolves there), and the pure
wheel and sdist must still be produced, carrying no binary.

A last journey installs the host's platform wheel, built around the real CLI,
into a fresh environment and runs ``scripts/verify-bundled-sdk.py`` — the check
the Windows PR lane and the release-time install proof run — so the bundled
binary is shown to be what the SDK resolves; the pure wheel is its failure path.
"""

from __future__ import annotations

import os
import platform
import shutil
import subprocess
import zipfile
from pathlib import Path

import pytest

PROJECT = Path(__file__).resolve().parents[1]
REPO_ROOT = PROJECT.parents[1]
MEMBERS = ("sdks/python", "plugins/pytest")
SCRIPTS = ("build-python-dist.sh", "build-python-wheel.sh", "verify-bundled-sdk.py")
SKILL = REPO_ROOT / "tests" / "fixtures" / "smoke" / "greeter"

#: Rust target -> (wheel platform tag, bundled file name). The release ships
#: these; release-platforms.toml states them and the release-target gate holds
#: the scripts to it.
PLATFORMS = {
    "x86_64-unknown-linux-gnu": ("manylinux_2_17_x86_64", "skilltest"),
    "aarch64-unknown-linux-gnu": ("manylinux_2_17_aarch64", "skilltest"),
    "x86_64-apple-darwin": ("macosx_10_12_x86_64", "skilltest"),
    "aarch64-apple-darwin": ("macosx_11_0_arm64", "skilltest"),
    "x86_64-pc-windows-msvc": ("win_amd64", "skilltest.exe"),
    "aarch64-pc-windows-msvc": ("win_arm64", "skilltest.exe"),
}

#: (system, machine) of a host this suite runs on -> its Rust target.
HOST_TARGETS = {
    ("Linux", "x86_64"): "x86_64-unknown-linux-gnu",
    ("Linux", "aarch64"): "aarch64-unknown-linux-gnu",
    ("Darwin", "x86_64"): "x86_64-apple-darwin",
    ("Darwin", "arm64"): "aarch64-apple-darwin",
}


# llmlint: ignore[code_lands_in_the_domain_that_owns_it] the scripts package this member's wheel
def run(
    command: list[str], cwd: Path, env: dict[str, str] | None = None
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(command, cwd=cwd, capture_output=True, text=True, env=env)


def copy_workspace(scratch: Path) -> None:
    """The uv workspace root, both members' tracked files and the packaging
    scripts — what the scripts read — so their builds cannot touch this tree."""
    for name in ("pyproject.toml", "uv.lock"):
        shutil.copy(REPO_ROOT / name, scratch / name)
    listed = subprocess.run(
        ["git", "ls-files", "-z", *MEMBERS], cwd=REPO_ROOT, check=True, capture_output=True
    )
    for name in filter(None, listed.stdout.decode().split("\0")):
        destination = scratch / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy(REPO_ROOT / name, destination)
    (scratch / "scripts").mkdir()
    for script in SCRIPTS:
        shutil.copy(REPO_ROOT / "scripts" / script, scratch / "scripts" / script)


def bin_members(wheel: Path) -> dict[str, bytes]:
    with zipfile.ZipFile(wheel) as zf:
        return {n: zf.read(n) for n in zf.namelist() if n.startswith("skilltest_sdk/_bin/")}


def wheel_tags(wheel: Path) -> list[str]:
    with zipfile.ZipFile(wheel) as zf:
        (meta,) = [n for n in zf.namelist() if n.endswith(".dist-info/WHEEL")]
        lines = zf.read(meta).decode().splitlines()
    return [line.removeprefix("Tag: ") for line in lines if line.startswith("Tag: ")]


# Not a marker-deselected tier: this runs unconditionally in the project's `test-e2e`
# target, which is its whole test tier by design; the builds are local (`uv build`,
# `uvx wheel`), as in test_wheel_typed.py.
# llmlint: ignore[test_tiers_split_by_project_not_by_marker, shell_test_tiers_stay_split] no nx project owns scripts/; they package this member's wheel, so they run in its own tier  # noqa: E501
def test_dist_builds_a_tagged_wheel_per_platform_plus_pure_wheel_and_sdist(
    tmp_path: Path,
) -> None:
    copy_workspace(tmp_path)
    binaries = tmp_path / "artifacts"
    for target, (_, exe) in PLATFORMS.items():
        stand_in = binaries / f"skilltest-{target}" / exe
        stand_in.parent.mkdir(parents=True)
        stand_in.write_bytes(f"stand-in for {target}".encode())
    out = tmp_path / "out"

    built = run(["bash", "scripts/build-python-dist.sh", "artifacts", str(out)], tmp_path)
    assert built.returncode == 0, built.stderr
    assert "6 platform wheel(s)" in built.stdout, built.stdout

    wheels = sorted(out.glob("*.whl"))
    by_tag = {w.name.rsplit("-", 1)[1].removesuffix(".whl"): w for w in wheels}
    assert sorted(by_tag) == sorted([tag for tag, _ in PLATFORMS.values()] + ["any"]), wheels
    for target, (tag, exe) in PLATFORMS.items():
        wheel = by_tag[tag]
        assert wheel_tags(wheel) == [f"py3-none-{tag}"], wheel.name
        assert bin_members(wheel) == {
            f"skilltest_sdk/_bin/{exe}": f"stand-in for {target}".encode()
        }, wheel.name

    assert bin_members(by_tag["any"]) == {}, "the pure wheel must bundle no binary"
    assert len(list(out.glob("skilltest_sdk-*.tar.gz"))) == 1, sorted(out.iterdir())
    assert not (tmp_path / "sdks/python/skilltest_sdk/_bin").exists(), "the build must clean _bin"


def test_wheel_script_refuses_a_target_with_no_platform_tag(tmp_path: Path) -> None:
    copy_workspace(tmp_path)
    stand_in = tmp_path / "skilltest"
    stand_in.write_bytes(b"stand-in")

    ran = run(
        ["bash", "scripts/build-python-wheel.sh", "x86_64-unknown-freebsd", str(stand_in), "out"],
        tmp_path,
    )

    assert ran.returncode == 2, ran.stderr
    assert "unsupported target: x86_64-unknown-freebsd" in ran.stderr
    assert not (tmp_path / "out").exists()


def fresh_install(tmp_path: Path, wheel: Path) -> Path:
    venv = tmp_path / "consumer"
    for command in (
        ["uv", "venv", "--quiet", str(venv)],
        ["uv", "pip", "install", "--quiet", "--python", str(venv / "bin" / "python"), str(wheel)],
    ):
        done = run(command, tmp_path)
        assert done.returncode == 0, done.stderr
    return venv / "bin" / "python"


def consumer_env() -> dict[str, str]:
    """The caller's environment without the gate's SKILLTEST_BIN, as a consumer has."""
    return {k: v for k, v in os.environ.items() if k not in ("SKILLTEST_BIN", "VIRTUAL_ENV")}


def host_target() -> str:
    target = HOST_TARGETS.get((platform.system(), platform.machine()))
    if target is None:
        pytest.fail(
            f"no release target is mapped for this host {platform.system()}/{platform.machine()}"
        )
    return target


def cli_version(cli: str) -> str:
    out = subprocess.run([cli, "--version"], check=True, capture_output=True, text=True)
    return out.stdout.strip().removeprefix("skilltest ")


# The SDK's test-e2e target exports SKILLTEST_BIN as the freshly built CLI; that is
# the binary bundled here. Installing resolves the wheel's dependencies through uv.
# llmlint: ignore[test_tiers_split_by_project_not_by_marker, shell_test_tiers_stay_split] the journey the Windows lane and release proof run, on this member's own tier  # noqa: E501
def test_installed_platform_wheel_runs_its_bundled_cli(tmp_path: Path) -> None:
    cli = os.environ.get("SKILLTEST_BIN")
    assert cli and Path(cli).is_file(), (
        "run through the project's test-e2e target, which builds the CLI"
    )
    copy_workspace(tmp_path)
    target = host_target()
    tag, _ = PLATFORMS[target]

    built = run(["bash", "scripts/build-python-wheel.sh", target, cli, "out"], tmp_path)
    assert built.returncode == 0, built.stderr
    (wheel,) = (tmp_path / "out").glob(f"*-py3-none-{tag}.whl")
    python = fresh_install(tmp_path, wheel)

    verified = run(
        [str(python), "scripts/verify-bundled-sdk.py", cli_version(cli), str(SKILL)],
        tmp_path,
        env=consumer_env(),
    )

    assert verified.returncode == 0, verified.stderr
    assert "_bin/skilltest" in verified.stdout, verified.stdout


# llmlint: ignore[test_tiers_split_by_project_not_by_marker, shell_test_tiers_stay_split] the failure path of the journey above, on the same tier  # noqa: E501
def test_verify_refuses_the_pure_wheel_which_bundles_no_cli(tmp_path: Path) -> None:
    copy_workspace(tmp_path)
    built = run(
        ["uv", "build", "--wheel", "--out-dir", str(tmp_path / "out")], tmp_path / "sdks/python"
    )
    assert built.returncode == 0, built.stderr
    (wheel,) = (tmp_path / "out").glob("*-py3-none-any.whl")
    python = fresh_install(tmp_path, wheel)

    verified = run(
        [str(python), "scripts/verify-bundled-sdk.py", "0.0.0", str(SKILL)],
        tmp_path,
        env=consumer_env(),
    )

    assert verified.returncode == 1
    assert "carries no" in verified.stderr and "install a platform wheel" in verified.stderr
