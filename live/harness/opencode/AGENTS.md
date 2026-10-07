# live/harness/opencode — the opencode live smoke

Nx project `skilltest-live-harness-opencode` (`type:live`, `lang:bash`): the
per-harness lane in `scripts/e2e-harness.sh`, fixed to `opencode`. The lane's
shared rules are in `live/harness/AGENTS.md`.

- **What it proves.** The whole pipeline against a real opencode: the built CLI
  through `oneharness`, judged by the fixed claude-code judge, then the harness's
  mock phase where its hooks allow.
- **Depends on.** `skilltest-cli` (its `live` target builds it first). Nothing
  may depend on a live project.
- **Run.** `just test-harness opencode` (`nx run skilltest-live-harness-opencode:live`);
  needs `oneharness`, the opencode CLI, `ANTHROPIC_API_KEY` and network. It runs from
  `.github/workflows/e2e-opencode.yml`, never from `just check`.
