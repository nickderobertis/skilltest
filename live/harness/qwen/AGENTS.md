# live/harness/qwen — the qwen live smoke

Nx project `skilltest-live-harness-qwen` (`type:live`, `lang:bash`): the
per-harness lane in `scripts/e2e-harness.sh`, fixed to `qwen`.

- **What it proves.** The whole pipeline against a real qwen: the built CLI
  through `oneharness`, judged by the fixed claude-code judge, then the harness's
  mock phase where its hooks allow.
- **Depends on.** `skilltest-cli` (its `live` target builds it first). Nothing
  may depend on a live project.
- **Run.** `just test-harness qwen` (`nx run skilltest-live-harness-qwen:live`);
  needs `oneharness`, the qwen CLI, `OPENAI_API_KEY` and network. It runs from
  `.github/workflows/e2e-qwen.yml`, never from `just check`.
