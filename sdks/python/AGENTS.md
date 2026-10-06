# sdks/python — `skilltest-sdk`

Nx project `skilltest-sdk` (`type:sdk`, `lang:python`). The Python SDK: a thin
wrapper that runs the `skilltest` CLI as a subprocess and parses its JSON
contract into Pydantic models. No framework code — pytest support lives in
`plugins/pytest`.

- **What it proves.** Its `test-e2e` target runs the pytest suite against the
  real built CLI and the fake provider (`SKILLTEST_BIN`/`SKILLTEST_PROVIDER`
  point at `target/debug`): `run_skill`, streaming, the case builders against the
  kitchen-sink golden, mocks/spies, binary and oneharness resolution, and the
  wheel's typing marker. `lint` (ruff), `format-check`, `typecheck` (ty).
- **Depends on.** `skilltest-cli` (the binary it wraps; `test-e2e` builds it
  first) and `skilltest-contract` (its `_*.py` models are generated from
  `schemas/`). Depended on by `skilltest-pytest`.
- **Run.** Its targets run through nx (`pnpm exec nx run skilltest-sdk:<target>`, any target its `project.json`
  declares); `just check` runs them whenever a change reaches this project.

## Rules for this package

- The public API (`run_skill`, `stream_skill`, `tool_calls`, the case builders)
  is re-exported from `skilltest_sdk/__init__.py`; everything else is internal.
  `run_skill`/`stream_skill` take a YAML path **or** a code-defined `TestCase`.
- The `case.py` builders (`TestCase`/`user`/`boolean`/`numeric`/`called`/
  `not_called`, reusing the `mock.py` builders) construct the **generated**
  `_case.py` models, so the payload cannot drift from the Rust parse; a
  code-defined case reaches the CLI through `--case-json`. Never hand-edit
  `_case.py`, `_report.py`, `_validation.py` or `_error.py` — `just gen-contract`.
- The CLI's own `--format json` output is the one input not re-validated by
  hand: the generated Pydantic models validate it for free.
- The wheel ships per target as a **platform wheel** bundling the CLI at
  `skilltest_sdk/_bin/skilltest` (plus a pure-wheel/sdist fallback), built by
  `scripts/build-python-wheel.sh`/`build-python-dist.sh` and proven by
  `bundle-smoke.yml`. The runner resolves the bundled binary first, falling back
  to `$SKILLTEST_BIN`/`PATH`.
- It depends on `oneharness-cli`, bounded to the release line
  `scripts/install-oneharness.sh` pins (`crates/skilltest-cli-e2e/tests/pins.rs`
  reconciles the two), so the default provider's `oneharness` comes with the
  install; the runner points the CLI at it via `SKILLTEST_ONEHARNESS_BIN` (a
  config `provider.bin` or a caller-set var still wins).
- This package and `plugins/pytest` are the two members of the uv workspace the
  root `pyproject.toml` declares (`[tool.uv.workspace]`, a virtual root that
  publishes nothing); both resolve into the one root `uv.lock`, never a lock per
  package. `scripts/set-version.sh` refreshes that lock on each release.
