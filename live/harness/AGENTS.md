# live/harness — the per-harness live smoke

Nx project `skilltest-live-harness` (`type:live`, `lang:bash`): `e2e-harness.sh`
and its helpers `e2e-lib.sh`, which drive the built `skilltest` CLI against one
**real** harness through `oneharness`, judged by a fixed claude-code judge, and
assert the run passes and the skill's reply surfaced — then the harness's
mock/stub phase where its hooks allow.

- **What it proves.** The whole pipeline per harness. The matrix is live-green
  and in CI: claude-code, codex, goose, opencode, cursor, crush, qwen, copilot —
  one workflow each (`.github/workflows/e2e-<id>.yml`), which `ci.yml` calls on
  every ordinary push to main ahead of the release, never on a pull request.
- **Depends on.** `skilltest-cli` (its `live` target builds it first). Nothing may
  depend on a live project.
- **Run.** `just test-harness <id>` (`nx run skilltest-live-harness:live
  --harness=<id>`); needs `oneharness`, the harness CLI, its secret and network.
  `docs/e2e.md` holds the matrix (models, per-harness delivery/extraction, the
  qwen gpt-5 gotcha), the secrets flow and the runbook for adding a harness.

## Rules

- oneharness v0.2.1+ delivers `--system` to every harness and v0.2.37+ extracts
  every harness's reply natively; add a harness to `e2e-lib.sh` only once it is
  validated live, else it stays a loud skip.
- Locally the script exits 0 on a skip (a harness that cannot run here is not a
  failure); the CI workflow is where the credential is required, and it fails
  fast without it. Keep the `type:live` tag: it is what keeps this project out of
  both `just check` tiers.
