# live/harness/copilot — the copilot live smoke

Nx project `skilltest-live-harness-copilot` (`type:live`, `lang:bash`): the
per-harness lane in `scripts/e2e-harness.sh`, fixed to `copilot`.

- **What it proves.** The whole pipeline against a real copilot: the built CLI
  through `oneharness`, judged by the fixed claude-code judge, then the harness's
  mock phase where its hooks allow.
- **Depends on.** `skilltest-cli` (its `live` target builds it first). Nothing
  may depend on a live project.
- **Run.** `just test-harness copilot` (`nx run skilltest-live-harness-copilot:live`);
  needs `oneharness`, the copilot CLI, `COPILOT_GITHUB_TOKEN` and network. It runs from
  `.github/workflows/e2e-copilot.yml`, never from `just check`.
