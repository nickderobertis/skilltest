"""Write the `--mocks` file the Python SDK writes (compile_decls + json.dump)
for every SDK mock verb, and the byte form the TypeScript SDK writes for the
same declarations (JSON.stringify, via node). Run from the repo root:

    uv run --project sdks/python python spikes/saphyr-diff/gen_sdk_mocks.py
"""

import json
import subprocess
from pathlib import Path

from skilltest_sdk.mock import deny, rewrite, spy, stub, compile_decls

OUT = Path(__file__).parent / "fixtures" / "sdk-mocks"

mocks = [
    spy(tool="bash", contains="ls"),
    stub("git push", output="Everything up-to-date"),
    stub(pattern=r"^make\b", output="done", exit_code=2, tool="bash", name="mk"),
    stub(
        "jobctl status",
        responses=["queued", {"output": "failed", "exit_code": 1}, "passed"],
    ),
    deny("rm -rf", message="destructive commands are blocked"),
    deny(tool="bash", where={"command": "git push --force"}, message="no force"),
    rewrite(
        tool="read",
        where={"file_path": "/etc/hosts"},
        input={
            "file_path": "fixtures/hosts",
            "zeta": 1,
            "alpha": [True, None, 1.5, "s", "yes", "010"],
            "nested": {"b": 2, "a": {"c": []}},
            "unicode": "café — ü",
            "big": 2**63,
        },
    ),
]
# A character outside the Basic Multilingual Plane: Python's json.dump writes
# it as a UTF-16 surrogate-pair escape (`\ud83d\ude00`), node writes it raw.
astral = [rewrite("cat", input={"command": "echo \U0001f600"})]


def write(name: str, decls: list) -> None:
    with (OUT / f"python{name}.json").open("w") as handle:
        json.dump(decls, handle)
    node = subprocess.run(
        ["node", "-e", "process.stdout.write(JSON.stringify(JSON.parse(require('fs').readFileSync(0,'utf8'))))"],
        input=json.dumps(decls),
        capture_output=True,
        text=True,
        check=True,
    )
    (OUT / f"typescript{name}.json").write_text(node.stdout)


OUT.mkdir(parents=True, exist_ok=True)
write("", compile_decls(mocks))
write("-astral", compile_decls(astral))
