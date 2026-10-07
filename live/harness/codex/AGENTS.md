# live/harness/codex — the codex live smoke

Nx project `skilltest-live-harness-codex` (`type:live`, `lang:bash`): the
per-harness lane in `scripts/e2e-harness.sh`, fixed to `codex`. The lane's
shared rules are in `live/harness/AGENTS.md`.

- **What it proves.** The whole pipeline against a real codex: the built CLI
  through `oneharness`, judged by the fixed claude-code judge, then the harness's
  mock phase where its hooks allow.
- **Depends on.** `skilltest-cli` (its `live` target builds it first). Nothing
  may depend on a live project.
- **Run.** `just test-harness codex` (`nx run skilltest-live-harness-codex:live`);
  needs `oneharness`, the codex CLI, `OPENAI_API_KEY` and network. It runs from
  `.github/workflows/e2e-codex.yml`, never from `just check`.
