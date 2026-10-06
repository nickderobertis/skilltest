# plugins/vitest — `@skill-test/vitest`

Nx project `@skill-test/vitest` (`type:plugin`, `lang:typescript`). The
`skillTest`/`discover` vitest helpers, built on — and re-exporting —
`@skill-test/sdk`.

- **What it proves.** Its `test-e2e` target runs vitest over the helpers against
  the real built CLI and the fake provider: discovery, code-defined cases, a
  failing case's report, mocks and stub sequences, and the SDK re-export.
  `build`/`typecheck` (tsc), `lint`/`format-check` (biome).
- **Depends on.** `@skill-test/sdk` only; it reaches the CLI through the SDK.
- **Run.** `pnpm exec nx run @skill-test/vitest:<build|test-e2e|lint|format-check|typecheck|format>`.

## Rules for this package

- It consumes `@skill-test/sdk` as a `workspace:*` dependency, which `pnpm
  publish` rewrites to the real version. Build the SDK before typechecking or
  testing this package: its `typecheck` depends on `^build`, and the `just`
  recipes go through nx, so they do.
- Public API is `src/index.ts`; no framework logic moves into the SDK.
