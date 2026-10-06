# live/harness — the per-harness live smoke

Nx project `skilltest-live-harness` (`type:live`, `lang:bash`). It owns the
per-harness lane in `scripts/e2e-harness.sh` and its helpers `scripts/e2e-lib.sh`
(declared as this project's inputs, so a change to either selects it): drive the
built `skilltest` CLI against one **real** harness through `oneharness`, judged
by a fixed claude-code judge, assert the run passes and the skill's reply
surfaced — then the harness's mock/stub phase where its hooks allow.

- **What it proves.** The whole pipeline, per harness. Each harness has its own
  workflow (`.github/workflows/e2e-<id>.yml`), which `ci.yml` calls on every
  ordinary push to main ahead of the release, never on a pull request.
- **Depends on.** `skilltest-cli` (its `live` target builds it first). Nothing may
  depend on a live project.
- **Run.** `just test-harness <id>` (`nx run skilltest-live-harness:live
  --harness=<id>`); needs `oneharness`, the harness CLI, its secret and network.
  `lint` runs shellcheck over both scripts, and every `just check` tier runs it.

## Rules

- Add a harness to `e2e-lib.sh` only once it is validated live, else it stays a
  loud skip.
- Locally the script exits 0 on a skip (a harness that cannot run here is not a
  failure); the CI workflow is where the credential is required, and it fails
  fast without it. Keep the `type:live` tag: it is what keeps this project out of
  both `just check` tiers.
