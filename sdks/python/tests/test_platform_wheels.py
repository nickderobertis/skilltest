"""The release's wheel scripts build one correctly tagged wheel per platform.

``publish.yml``'s PyPI job runs ``scripts/build-python-dist.sh`` over the
per-target binaries, which calls ``scripts/build-python-wheel.sh`` once per
target, then builds the pure wheel and sdist. These tests run those scripts
as the job does — over a scratch copy of the uv workspace, so nothing is
written into this tree — with a stand-in file per target, and read the zips:
each platform wheel must carry its platform tag and exactly its bundled binary
(``skilltest.exe`` on Windows, the name the runner resolves there), and the pure
wheel and sdist must still be produced, carrying no binary.

The rest install the host's platform wheel, built around the real CLI, into a
fresh environment and drive ``scripts/verify_bundled.py`` — the check the
Windows PR lane and the release-time install proof run — through its pass and
through each way it must refuse.
"""

from __future__ import annotations

import os
import platform
import shutil
import subprocess
import zipfile
from pathlib import Path
from typing import NamedTuple

import pytest

PROJECT = Path(__file__).resolve().parents[1]
REPO_ROOT = PROJECT.parents[1]
MEMBERS = ("sdks/python", "plugins/pytest")
PACKAGING = ("build-python-dist.sh", "build-python-wheel.sh")
VERIFY = PROJECT / "scripts" / "verify_bundled.py"
SKILLS = REPO_ROOT / "tests" / "fixtures" / "skills"


class Platform(NamedTuple):
    """One platform the release ships a wheel for (release-platforms.toml)."""

    target: str
    wheel_tag: str
    exe: str


class Host(NamedTuple):
    system: str
    machine: str


PLATFORMS = (
    Platform("x86_64-unknown-linux-gnu", "manylinux_2_17_x86_64", "skilltest"),
    Platform("aarch64-unknown-linux-gnu", "manylinux_2_17_aarch64", "skilltest"),
    Platform("x86_64-apple-darwin", "macosx_10_12_x86_64", "skilltest"),
    Platform("aarch64-apple-darwin", "macosx_11_0_arm64", "skilltest"),
    Platform("x86_64-pc-windows-msvc", "win_amd64", "skilltest.exe"),
    Platform("aarch64-pc-windows-msvc", "win_arm64", "skilltest.exe"),
)

#: The hosts this suite runs on, mapped to the platform whose wheel they install.
HOST_PLATFORMS = {
    Host("Linux", "x86_64"): PLATFORMS[0],
    Host("Linux", "aarch64"): PLATFORMS[1],
    Host("Darwin", "x86_64"): PLATFORMS[2],
    Host("Darwin", "arm64"): PLATFORMS[3],
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
    for script in PACKAGING:
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
    for plat in PLATFORMS:
        stand_in = binaries / f"skilltest-{plat.target}" / plat.exe
        stand_in.parent.mkdir(parents=True)
        stand_in.write_bytes(f"stand-in for {plat.target}".encode())
    out = tmp_path / "out"

    built = run(["bash", "scripts/build-python-dist.sh", "artifacts", str(out)], tmp_path)
    assert built.returncode == 0, built.stderr
    assert "6 platform wheel(s)" in built.stdout, built.stdout

    wheels = sorted(out.glob("*.whl"))
    by_tag = {w.name.rsplit("-", 1)[1].removesuffix(".whl"): w for w in wheels}
    assert sorted(by_tag) == sorted([p.wheel_tag for p in PLATFORMS] + ["any"]), wheels
    for plat in PLATFORMS:
        wheel = by_tag[plat.wheel_tag]
        assert wheel_tags(wheel) == [f"py3-none-{plat.wheel_tag}"], wheel.name
        assert bin_members(wheel) == {
            f"skilltest_sdk/_bin/{plat.exe}": f"stand-in for {plat.target}".encode()
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


def fresh_install(scratch: Path, wheel: Path) -> Path:
    venv = scratch / "consumer"
    for command in (
        ["uv", "venv", "--quiet", str(venv)],
        ["uv", "pip", "install", "--quiet", "--python", str(venv / "bin" / "python"), str(wheel)],
    ):
        done = run(command, scratch)
        assert done.returncode == 0, done.stderr
    return venv / "bin" / "python"


def consumer_env(extra_path: Path | None = None) -> dict[str, str]:
    """The caller's environment without the gate's SKILLTEST_BIN, as a consumer has."""
    env = {k: v for k, v in os.environ.items() if k not in ("SKILLTEST_BIN", "VIRTUAL_ENV")}
    if extra_path is not None:
        env["PATH"] = f"{extra_path}{os.pathsep}{env.get('PATH', '')}"
    return env


class Consumer(NamedTuple):
    python: Path
    version: str


@pytest.fixture(scope="module")
def consumer(tmp_path_factory: pytest.TempPathFactory) -> Consumer:
    """The host's platform wheel, built by the release script around the CLI the
    project's test-e2e target exports as SKILLTEST_BIN, installed fresh."""
    cli = os.environ.get("SKILLTEST_BIN")
    assert cli and Path(cli).is_file(), "run through the project's test-e2e target"
    plat = HOST_PLATFORMS.get(Host(platform.system(), platform.machine()))
    if plat is None:
        pytest.fail(f"no release platform is mapped for {platform.system()}/{platform.machine()}")
    scratch = tmp_path_factory.mktemp("host-wheel")
    copy_workspace(scratch)
    built = run(["bash", "scripts/build-python-wheel.sh", plat.target, cli, "out"], scratch)
    assert built.returncode == 0, built.stderr
    (wheel,) = (scratch / "out").glob(f"*-py3-none-{plat.wheel_tag}.whl")
    version = subprocess.run([cli, "--version"], check=True, capture_output=True, text=True)
    return Consumer(
        fresh_install(scratch, wheel), version.stdout.strip().removeprefix("skilltest ")
    )


def verify(
    python: Path, *args: str, env: dict[str, str] | None = None
) -> subprocess.CompletedProcess[str]:
    return run([str(python), str(VERIFY), *args], REPO_ROOT, env=env or consumer_env())


# Installing resolves the wheel's dependencies through uv, as a consumer's install does.
# llmlint: ignore[test_tiers_split_by_project_not_by_marker, shell_test_tiers_stay_split] the journey the Windows lane and release proof run, on this member's own tier  # noqa: E501
def test_installed_platform_wheel_runs_its_bundled_cli(consumer: Consumer) -> None:
    verified = verify(consumer.python, consumer.version, str(SKILLS / "greeter"))

    assert verified.returncode == 0, verified.stderr
    assert f"skilltest {consumer.version} bundled at" in verified.stdout, verified.stdout
    assert "_bin/skilltest" in verified.stdout, verified.stdout


def test_verify_refuses_a_bundle_of_another_release(consumer: Consumer) -> None:
    verified = verify(consumer.python, "9.9.9", str(SKILLS / "greeter"))

    assert verified.returncode == 1
    assert "not 'skilltest 9.9.9'" in verified.stderr
    assert "install skilltest-sdk==9.9.9" in verified.stderr


def test_verify_refuses_when_a_skilltest_on_path_could_answer(
    consumer: Consumer, tmp_path: Path
) -> None:
    impostor = tmp_path / "skilltest"
    impostor.write_text("#!/bin/sh\necho skilltest 0.0.0\n")
    impostor.chmod(0o755)

    verified = verify(
        consumer.python,
        consumer.version,
        str(SKILLS / "greeter"),
        env=consumer_env(extra_path=tmp_path),
    )

    assert verified.returncode == 1
    assert f"a skilltest is on PATH ({impostor})" in verified.stderr


def test_verify_refuses_a_skill_the_bundled_cli_finds_invalid(consumer: Consumer) -> None:
    verified = verify(consumer.python, consumer.version, str(SKILLS / "invalid"))

    assert verified.returncode == 1
    assert "missing a non-empty `description`" in verified.stderr


def test_verify_refuses_malformed_arguments(consumer: Consumer, tmp_path: Path) -> None:
    for args in ((consumer.version,), (consumer.version, str(tmp_path / "absent"))):
        verified = verify(consumer.python, *args)

        assert verified.returncode == 1, args
        assert "usage: verify_bundled.py <expected-version> <skill-dir>" in verified.stderr


# llmlint: ignore[test_tiers_split_by_project_not_by_marker, shell_test_tiers_stay_split] the failure path of the journey above, on the same tier  # noqa: E501
def test_verify_refuses_the_pure_wheel_which_bundles_no_cli(tmp_path: Path) -> None:
    copy_workspace(tmp_path)
    out = tmp_path / "out"
    built = run(["uv", "build", "--wheel", "--out-dir", str(out)], tmp_path / "sdks/python")
    assert built.returncode == 0, built.stderr
    (wheel,) = out.glob("*-py3-none-any.whl")
    python = fresh_install(tmp_path, wheel)

    verified = verify(python, "0.0.0", str(SKILLS / "greeter"))

    assert verified.returncode == 1
    assert "carries no" in verified.stderr and "py3-none-any" in verified.stderr
