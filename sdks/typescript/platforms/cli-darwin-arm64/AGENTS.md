# sdks/typescript/platforms/cli-darwin-arm64 — `@skill-test/cli-darwin-arm64`

Nx project `@skill-test/cli-darwin-arm64` (`type:carrier`, `lang:typescript`):
the npm package that carries the prebuilt `skilltest` binary for darwin-arm64,
`os`/`cpu`-scoped so a host installs only its own. It has no source and no
targets, so it proves nothing itself; `bundle-smoke.yml` proves the binary it
ships runs.

- **Depends on.** Nothing. `@skill-test/sdk` depends on it (an
  `optionalDependencies` entry pinned `workspace:*`).
- **Rules.** `bin/` is git-ignored and filled at publish time by
  `scripts/stage-npm-binary.sh` (`publish.yml`); only `bin/` ships (`files`).
  The version moves in lockstep through `scripts/set-version.sh` — never by hand.
