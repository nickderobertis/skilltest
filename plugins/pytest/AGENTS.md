# plugins/pytest — `skilltest-pytest`

Nx project `skilltest-pytest` (`type:plugin`, `lang:python`). pytest collection
of `*.skilltest.yaml` files as test items, built on — and re-exporting —
`skilltest-sdk`.

- **What it proves.** Its `test-e2e` target runs pytest over the plugin against
  the real built CLI and the fake provider: collection, a failing case's report,
  the publish step, the uv workspace's single lock (including that
  `scripts/set-version.sh` moves both members and the lock), and the wheel's
  typing marker. `lint` (ruff), `format-check`, `typecheck` (ty).
- **Depends on.** `skilltest-sdk` only; it reaches the CLI through the SDK.
- **Run.** `pnpm exec nx run skilltest-pytest:<test-e2e|lint|format-check|typecheck|format>`.

## Rules for this package

- `skilltest-pytest` consumes `skilltest-sdk` from the uv workspace in dev (the
  `[tool.uv.sources]` entry) and by an exact `skilltest-sdk==X.Y.Z` pin when
  published; `scripts/set-version.sh` keeps that pin in lockstep. PyPI builds
  from `[project]` metadata, so the workspace source never reaches the wheel.
- Settings come from `pytest.ini`/`pyproject.toml` (`skilltest_bin`,
  `skilltest_provider`, `skilltest_platforms`, `skilltest_models`,
  `skilltest_config`) or the `SKILLTEST_BIN`/`SKILLTEST_PROVIDER` env vars.
- Public API is re-exported from `skilltest_pytest/__init__.py`; no framework
  logic moves into the SDK.
