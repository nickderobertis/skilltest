"""Prove an installed `skilltest-sdk` runs the CLI bundled in its own wheel.

    <consumer-python> sdks/python/scripts/verify_bundled.py <expected-version> <skill-dir>

Run it with the interpreter of a fresh environment the wheel (local or from
PyPI) was installed into. With $SKILLTEST_BIN unset and no `skilltest` on PATH,
the SDK's own resolution must land on `skilltest_sdk/_bin/skilltest` —
`skilltest.exe` on Windows — inside the installed package; that binary must
report `skilltest <expected-version>`, and `validate_skill` must run through it
and find <skill-dir> valid. A pass can only come from the bundled binary.

Used by windows-build.yml (the PR lane, a locally built wheel) and publish.yml's
release-time install proof (the published wheel). Not shipped: the wheel packs
only `skilltest_sdk/`. Quiet on success: one line.
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
from pathlib import Path
from typing import NoReturn


def fail(message: str) -> NoReturn:
    print(f"verify-bundled: {message}", file=sys.stderr)
    sys.exit(1)


def main(argv: list[str]) -> None:
    if len(argv) != 2 or not argv[0] or not Path(argv[1]).is_dir():
        fail(
            "usage: verify_bundled.py <expected-version> <skill-dir>; pass the release "
            "version and an existing skill directory"
        )
    expected_version, skill_dir = argv

    os.environ.pop("SKILLTEST_BIN", None)
    on_path = shutil.which("skilltest")
    if on_path:
        fail(
            f"a skilltest is on PATH ({on_path}), so a pass could not prove the bundle is "
            "used; drop its directory from PATH and rerun"
        )

    try:
        import skilltest_sdk
        from skilltest_sdk import runner
    except ImportError as error:
        fail(
            f"skilltest_sdk does not import in {sys.executable} ({error}); install "
            "skilltest-sdk into this environment and rerun"
        )

    exe = "skilltest.exe" if os.name == "nt" else "skilltest"
    bundled = Path(skilltest_sdk.__file__).resolve().parent / "_bin" / exe
    if not bundled.is_file():
        fail(
            f"the installed wheel carries no {bundled}; install this platform's "
            "skilltest-sdk wheel (not the py3-none-any one) and rerun"
        )

    resolved = runner._resolve_bin(None)
    if Path(resolved).resolve() != bundled:
        fail(
            f"the SDK resolved {resolved!r} instead of its bundled {bundled}; fix "
            "skilltest_sdk.runner._resolve_bin's precedence"
        )

    try:
        ran = subprocess.run([resolved, "--version"], capture_output=True, text=True)
    except OSError as error:
        fail(
            f"{resolved} does not start ({error}); the bundled binary does not run on "
            "this host, so rebuild the wheel for its target"
        )
    if ran.returncode != 0:
        fail(
            f"{resolved} --version exited {ran.returncode} ({ran.stderr.strip()}); the "
            "bundled binary does not run on this host, so rebuild the wheel for its target"
        )
    if ran.stdout.strip() != f"skilltest {expected_version}":
        fail(
            f"{resolved} --version said {ran.stdout.strip()!r}, not 'skilltest "
            f"{expected_version}'; install skilltest-sdk=={expected_version}, or rebuild its "
            "wheel from that release's binary"
        )

    # The except is broad on purpose: whatever the SDK raises here is this check's finding.
    try:
        report = skilltest_sdk.validate_skill(skill_dir)
    except Exception as error:
        fail(
            f"validate_skill({skill_dir!r}) through the bundled CLI raised {error!r}; "
            "rerun it by hand with the same interpreter to see the full error"
        )
    if not report.valid:
        fail(
            f"validate_skill({skill_dir!r}) through the bundled CLI found {report.findings}; "
            "point this check at a valid skill such as tests/fixtures/smoke/greeter"
        )

    print(f"verify-bundled: {ran.stdout.strip()} bundled at {bundled}")


if __name__ == "__main__":
    main(sys.argv[1:])
