# live/harness/claude-code — the claude-code live smoke

Nx project `skilltest-live-harness-claude-code` (`type:live`, `lang:bash`): the
per-harness lane in `scripts/e2e-harness.sh`, fixed to `claude-code`.

- **What it proves.** The whole pipeline against a real claude-code: the built CLI
  through `oneharness`, judged by the fixed claude-code judge, then the harness's
  mock phase where its hooks allow.
- **Depends on.** `skilltest-cli` (its `live` target builds it first). Nothing
  may depend on a live project.
- **Run.** `just test-harness claude-code` (`nx run skilltest-live-harness-claude-code:live`);
  needs `oneharness`, the claude-code CLI, `CLAUDE_CODE_OAUTH_TOKEN` and network. It runs from
  the `just test-claude` convenience recipe locally; CI's `e2e-claude.yml` runs the deeper `skilltest-live-claude` suite instead, never from `just check`.
