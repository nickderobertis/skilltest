"""Prove an installed `skilltest-sdk` runs the CLI bundled in its own wheel.

    <consumer-python> scripts/verify-bundled-sdk.py <expected-version> <skill-dir>

Run it with the interpreter of a fresh environment the wheel (local or from
PyPI) was installed into. With $SKILLTEST_BIN unset and no `skilltest` on PATH,
the SDK's own resolution must land on `skilltest_sdk/_bin/skilltest` —
`skilltest.exe` on Windows — inside the installed package; that binary must
report `skilltest <expected-version>`, and `validate_skill` must run through it
and find <skill-dir> valid. A pass can only come from the bundled binary.

Used by windows-build.yml (the PR lane, a locally built wheel) and publish.yml's
release-time install proof (the published wheel). Quiet on success: one line.
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
from pathlib import Path


def fail(message: str) -> None:
    print(f"verify-bundled-sdk: {message}", file=sys.stderr)
    sys.exit(1)


def main() -> None:
    if len(sys.argv) != 3:
        fail("usage: verify-bundled-sdk.py <expected-version> <skill-dir>")
    expected_version, skill_dir = sys.argv[1], sys.argv[2]

    os.environ.pop("SKILLTEST_BIN", None)
    on_path = shutil.which("skilltest")
    if on_path:
        fail(f"a skilltest is on PATH ({on_path}), so a pass could not prove the bundle is used")

    import skilltest_sdk
    from skilltest_sdk import runner

    exe = "skilltest.exe" if os.name == "nt" else "skilltest"
    bundled = Path(skilltest_sdk.__file__).resolve().parent / "_bin" / exe
    if not bundled.is_file():
        fail(f"the installed wheel carries no {bundled}; install a platform wheel")

    resolved = runner._resolve_bin(None)
    if Path(resolved).resolve() != bundled:
        fail(f"the SDK resolved {resolved!r} instead of its bundled {bundled}")

    ran = subprocess.run([resolved, "--version"], capture_output=True, text=True)
    if ran.returncode != 0:
        fail(f"{resolved} --version exited {ran.returncode}: {ran.stderr.strip()}")
    if ran.stdout.strip() != f"skilltest {expected_version}":
        fail(
            f"{resolved} --version said {ran.stdout.strip()!r}, not 'skilltest {expected_version}'"
        )

    report = skilltest_sdk.validate_skill(skill_dir)
    if not report.valid:
        fail(f"validate_skill({skill_dir!r}) through the bundled CLI was not ok: {report}")

    print(f"verify-bundled-sdk: {ran.stdout.strip()} bundled at {bundled}")


if __name__ == "__main__":
    main()
