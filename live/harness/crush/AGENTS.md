# live/harness/crush — the crush live smoke

Nx project `skilltest-live-harness-crush` (`type:live`, `lang:bash`): the
per-harness lane in `scripts/e2e-harness.sh`, fixed to `crush`. The lane's
shared rules are in `live/harness/AGENTS.md`.

- **What it proves.** The whole pipeline against a real crush: the built CLI
  through `oneharness`, judged by the fixed claude-code judge, then the harness's
  mock phase where its hooks allow.
- **Depends on.** `skilltest-cli` (its `live` target builds it first). Nothing
  may depend on a live project.
- **Run.** `just test-harness crush` (`nx run skilltest-live-harness-crush:live`);
  needs `oneharness`, the crush CLI, `ANTHROPIC_API_KEY` and network. It runs from
  `.github/workflows/e2e-crush.yml`, never from `just check`.
