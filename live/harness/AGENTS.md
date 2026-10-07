# live/harness — the per-harness live smoke lane

Not a project itself: each subdirectory is one `type:live` Nx project,
`skilltest-live-harness-<id>`, whose `live` target runs the shared lane
`scripts/e2e-harness.sh` (helpers in `scripts/e2e-lib.sh`) fixed to that
harness. The scripts are every lane project's inputs, so a change to them
selects all eight. Each lane drives the built `skilltest` CLI against one
**real** harness through `oneharness`, judged by a fixed claude-code judge,
asserts the run passes and the skill's reply surfaced, then runs the harness's
mock/stub phase where its hooks allow.

## Rules

- One project per harness: adding a harness means its case in `e2e-lib.sh`, a
  `live/harness/<id>/` project (copy a sibling: `project.json` tagged
  `type:live`, `lang:bash`, plus its `AGENTS.md`) and its
  `.github/workflows/e2e-<id>.yml`. Keep the `type:live` tag: it is what keeps
  the project out of both `just check` tiers, with no recipe edit.
- Add a harness only once it is validated live, else it stays a loud skip.
- Locally the script exits 0 on a skip (a harness that cannot run here is not a
  failure); the CI workflow is where the credential is required, and it fails
  fast without it.
