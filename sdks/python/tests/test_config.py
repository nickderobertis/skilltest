"""Full/partial contract parity through the public SDK and built CLI."""

from __future__ import annotations

import asyncio
import importlib.util
import json
import os
import subprocess
from pathlib import Path

import pytest
from pydantic import TypeAdapter

from skilltest_sdk import Config, ConfigOverrides, SkilltestUsageError, run_skill, stream_skill


def test_schema_leaves_reach_yaml_set_environment_and_typed_overrides(
    fixtures: Path, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    root = fixtures.parents[1]
    spec = importlib.util.spec_from_file_location(
        "config_surfaces", fixtures / "contract/config_surfaces.py"
    )
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    schema = json.loads((root / "schemas/config.schema.json").read_text())
    adapter = TypeAdapter(ConfigOverrides)
    boundary = tmp_path / "oneharness"
    boundary.write_text(
        "#!/bin/sh\ncat >/dev/null\n"
        'printf \'%s\\n\' \'{"results":[{"status":"ok","text":"hello"}]}\'\n'
    )
    boundary.chmod(0o755)
    case = tmp_path / "case.json"
    case.write_text(
        json.dumps(
            {
                "skill": str(fixtures / "skills/greeter"),
                "input": "hello",
                "mocks": [{"name": "probe", "match": {"tool": "never"}}],
                "evals": [{"type": "not_called", "mock": "probe"}],
            }
        )
    )
    # The public SDK takes a YAML case; JSON is valid YAML too.
    monkeypatch.delenv("SKILLTEST_PROVIDER", raising=False)
    command = str(Path(os.environ["SKILLTEST_BIN"]).with_name("skilltest-fake-provider"))
    observed = set()
    for path, value, contexts in module.examples(schema):
        payload: dict = {"provider": {"kind": "oneharness", "bin": str(boundary), "history": False}}
        for prefix, context in contexts.items():
            if not prefix:
                continue
            cursor = payload
            parts = prefix.split(".")
            for part in parts[:-1]:
                cursor = cursor.setdefault(part, {})
            cursor[parts[-1]] = context.copy()
        # Values that name process boundaries stay valid and portable.
        provider = payload.setdefault("provider", {})
        if provider.get("kind") == "command":
            provider["command"] = [command]
        else:
            provider["bin"] = str(boundary)
        if path == "provider.bin":
            value = str(boundary)
        if path == "provider.command":
            value = [command]
        if path in ("platforms", "models"):
            value = ["surface"]
        cursor = payload
        parts = path.split(".")
        for part in parts[:-1]:
            cursor = cursor.setdefault(part, {})
        cursor[parts[-1]] = value
        typed = adapter.validate_python(payload)
        config = tmp_path / "config.yaml"
        config.write_text(json.dumps(payload))
        full = Config.model_validate(payload)
        assert full.model_dump(exclude_unset=True)
        report = run_skill(case, config=config)
        assert report.passed, path
        report = run_skill(case, config_overrides=typed)
        assert report.passed, path
        # Use the file as variant context, replacing only the derived leaf.
        argv = [
            os.environ["SKILLTEST_BIN"],
            "--config",
            str(config),
            "run",
            str(case),
            "--set",
            f"{path}={json.dumps(value)}",
        ]
        result = subprocess.run(argv, capture_output=True, text=True, check=False)
        assert result.returncode == 0, (path, result.stderr)
        name = "SKILLTEST_" + path.replace(".", "__").upper()
        result = subprocess.run(
            argv[:-2],
            env={**os.environ, name: json.dumps(value)},
            capture_output=True,
            text=True,
            check=False,
        )
        assert result.returncode == 0, (path, result.stderr)
        observed.add(path)
    assert {
        "provider.command",
        "provider.judge.config",
        "provider.simulated_user.args",
        "judge.strict_json",
        "provider.history_dir",
    } <= observed


def test_buffered_and_streaming_overrides_keep_explicit_values(cases: Path) -> None:
    overrides: ConfigOverrides = {"spy": False, "mocks": [], "judge": None, "max_turns": 2}
    assert run_skill(cases / "greet_pass.yaml", config_overrides=overrides).passed

    async def collect() -> None:
        stream = stream_skill(cases / "greet_pass.yaml", config_overrides=overrides)
        async for _ in stream:
            pass
        assert stream.report and stream.report.passed

    asyncio.run(collect())
    with pytest.raises(SkilltestUsageError, match="max_turns"):
        run_skill(cases / "greet_pass.yaml", config_overrides={"max_turns": 0})
