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
    capture = tmp_path / "argv.json"
    wrapper = tmp_path / "capture-cli"
    wrapper.write_text(
        "#!/usr/bin/env python3\n"
        "import json, os, sys\n"
        "with open(os.environ['SDK_CAPTURE_PATH'], 'w') as output:\n"
        "    json.dump(sys.argv[1:], output)\n"
        "binary = os.environ['SDK_REAL_CLI']\n"
        "os.execv(binary, [binary, *sys.argv[1:]])\n"
    )
    wrapper.chmod(0o755)
    monkeypatch.setenv("SDK_CAPTURE_PATH", str(capture))
    monkeypatch.setenv("SDK_REAL_CLI", os.environ["SKILLTEST_BIN"])
    # The wrapper only observes argv, then replaces itself with the real CLI.
    # Even an induced serialization bug stays on this scripted provider boundary.
    monkeypatch.setenv("SKILLTEST_ONEHARNESS_BIN", str(boundary))
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
        report = run_skill(case, bin=wrapper, config_overrides=typed)
        assert report.passed, path
        captured = json.loads(capture.read_text())
        delivered = {}
        for index, token in enumerate(captured):
            if token == "--set":
                key, encoded = captured[index + 1].split("=", 1)
                delivered[key] = json.loads(encoded)
        assert delivered == payload, path
        # Core's schema-leaf suite asserts the applied values; this real-process
        # edge proves the SDK delivers every leaf without bypassing that resolver.
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


def test_documented_config_loads_through_the_cli(fixtures: Path, tmp_path: Path) -> None:
    import re

    root = fixtures.parents[1]
    documented = (root / "docs/schema.md").read_text().split("## Test case")[0]
    for block in re.findall(r"```yaml\n(.*?)```", documented, re.DOTALL):
        # The CLI's YAML loader remains the authority; a provider-boundary error
        # from missing example role files comes only after configuration loads.
        config = tmp_path / "example.yaml"
        config.write_text(block)
        result = subprocess.run(
            [os.environ["SKILLTEST_BIN"], "--config", str(config), "run"],
            capture_output=True,
            text=True,
            check=False,
        )
        assert "no test" in result.stderr or "no cases" in result.stderr, result.stderr


def test_api_judge_overrides_reach_http_for_buffered_and_streaming(
    cases: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    import threading
    from http.server import BaseHTTPRequestHandler, HTTPServer

    requests: list[dict] = []

    class Handler(BaseHTTPRequestHandler):
        def do_POST(self) -> None:
            assert self.path == "/judge"
            assert self.headers["Authorization"] == "Bearer fixture-key"
            body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            requests.append(body)
            reply = json.dumps(
                {
                    "choices": [
                        {
                            "message": {
                                "content": (
                                    '{"value":false} reconsidering '
                                    '{"value":true,"reason":"revised API verdict"}'
                                )
                            }
                        }
                    ],
                    "usage": {"prompt_tokens": 11, "completion_tokens": 2},
                }
            ).encode()
            self.send_response(200)
            self.send_header("Content-Length", str(len(reply)))
            self.end_headers()
            self.wfile.write(reply)

        def log_message(self, format: str, *args: object) -> None:
            pass

    server = HTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever)
    thread.start()
    monkeypatch.setenv("CONFIG_TEST_API_KEY", "fixture-key")
    overrides: ConfigOverrides = {
        "judge": {
            "kind": "api",
            "vendor": "openai",
            "api_key_env": "CONFIG_TEST_API_KEY",
            "base_url": f"http://127.0.0.1:{server.server_port}/judge",
            "timeout_secs": 10,
            "curl_bin": "curl",
            "strict_json": False,
        },
        "judge_model": "config-api-model",
    }
    try:
        report = run_skill(cases / "greet_pass.yaml", config_overrides=overrides)
        assert report.passed and report.runs[0].usage
        assert report.runs[0].usage.input_tokens == 22

        async def collect() -> None:
            stream = stream_skill(cases / "greet_pass.yaml", config_overrides=overrides)
            async for _ in stream:
                pass
            assert stream.report and stream.report.passed

        asyncio.run(collect())
        assert len(requests) == 4
        assert all(
            request["model"] == "config-api-model" and "response_format" not in request
            for request in requests
        )
    finally:
        server.shutdown()
        thread.join()
        server.server_close()


def test_generated_config_model_omits_unset_defaults_at_the_real_cli(
    cases: Path, tmp_path: Path
) -> None:
    from typing import cast

    from pydantic import ValidationError

    config = tmp_path / "inherited.yaml"
    config.write_text("platforms: [inherited-platform]\nmodels: [inherited-model]\n")
    model = Config.model_validate({"spy": False, "mocks": [], "judge": None})
    assert model.model_dump(mode="json", exclude_unset=True) == {
        "spy": False,
        "mocks": [],
        "judge": None,
    }
    # The serializer accepts generated BaseModel payloads as well as typed
    # partial dictionaries; both have precisely the Config property names.
    report = run_skill(
        cases / "greet_pass.yaml",
        config=config,
        config_overrides=cast("ConfigOverrides", model),
    )
    assert report.passed
    assert report.runs[0].platform == "inherited-platform"
    assert report.runs[0].model == "inherited-model"
    with pytest.raises(ValidationError):
        Config.model_validate({"max_turns": None})
