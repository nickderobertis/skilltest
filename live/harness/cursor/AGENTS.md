# live/harness/cursor — the cursor live smoke

Nx project `skilltest-live-harness-cursor` (`type:live`, `lang:bash`): the
per-harness lane in `scripts/e2e-harness.sh`, fixed to `cursor`.

- **What it proves.** The whole pipeline against a real cursor: the built CLI
  through `oneharness`, judged by the fixed claude-code judge, then the harness's
  mock phase where its hooks allow.
- **Depends on.** `skilltest-cli` (its `live` target builds it first). Nothing
  may depend on a live project.
- **Run.** `just test-harness cursor` (`nx run skilltest-live-harness-cursor:live`);
  needs `oneharness`, the cursor CLI, `CURSOR_API_KEY` and network. It runs from
  `.github/workflows/e2e-cursor.yml`, never from `just check`.
