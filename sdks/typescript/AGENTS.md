# sdks/typescript — `@skill-test/sdk`

Nx project `@skill-test/sdk` (`type:sdk`, `lang:typescript`). The TypeScript SDK:
the same thin CLI wrapper as the Python SDK, with type declarations generated
from the contract. No framework code — vitest support lives in `plugins/vitest`.

- **What it proves.** Its `test-e2e` target runs the vitest suite against the
  real built CLI and the fake provider: `runSkill`, streaming, the case builders
  against the kitchen-sink golden, mocks/spies through the real oneharness seam,
  and binary resolution. `build`/`typecheck` (tsc), `lint`/`format-check`
  (biome, one root config).
- **Depends on.** `skilltest-cli` (the binary it wraps), `skilltest-contract`
  (`src/generated/*.ts`) and the four `@skill-test/cli-*` carrier packages it
  declares as `optionalDependencies`. Depended on by `@skill-test/vitest`.
- **Run.** `pnpm exec nx run @skill-test/sdk:<build|test-e2e|lint|format-check|typecheck|format>`.

## Rules for this package

- Public API is `src/index.ts`: `runSkill` (YAML path **or** a code-defined
  case), the async streaming API `streamSkill` → `SkillStream` (a `for await` of
  tool events that `break`s to short-circuit), `toolCalls`/`ToolEvent`, and the
  `case.ts` builders (`testCase`/`user`/`boolean`/`numeric`/`called`/
  `notCalled`), typed against the **generated** `src/generated/case.ts`. Never
  hand-edit `src/generated/*` — `just gen-contract`.
- The CLI ships inside the per-platform `@skill-test/cli-*` packages
  (`platforms/`), declared as `optionalDependencies` so `pnpm add` pulls only the
  matching host's binary; the runner resolves it, falling back to
  `$SKILLTEST_BIN`/`PATH`.
- It depends on `oneharness-cli`, bounded to the same release line as the Python
  SDK. The runner resolves the **native** binary in the host's
  `@oneharness/cli-*` package (execing it directly, never the `oneharness-cli`
  node launcher) and points the CLI at it via `SKILLTEST_ONEHARNESS_BIN` (a
  config `provider.bin` or a caller-set var still wins), falling back to
  `oneharness` on `PATH` — `node_modules/.bin` need not be on `PATH`.
- The pnpm workspace is rooted at the repo (`pnpm-workspace.yaml`, one
  `pnpm-lock.yaml`); see the root AGENTS.md for why it is pnpm.
