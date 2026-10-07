# sdks/typescript/platforms/cli-darwin-x64 — `@skill-test/cli-darwin-x64`

Nx project `@skill-test/cli-darwin-x64` (`type:carrier`, `lang:typescript`): the
npm package that carries the prebuilt `skilltest` binary for darwin-x64,
`os`/`cpu`-scoped so a host installs only its own. It has no source and no
targets, so it proves nothing itself. Unlike the other three carriers, its
binary is built and published but not smoked by `bundle-smoke.yml` (the
Intel-macOS runner queues unreliably); the release tag's build is its proof.

- **Depends on.** Nothing. `@skill-test/sdk` depends on it (an
  `optionalDependencies` entry pinned `workspace:*`).
- **Rules.** `bin/` is git-ignored and filled at publish time by
  `scripts/stage-npm-binary.sh` (`publish.yml`); only `bin/` ships (`files`).
  The version moves in lockstep through `scripts/set-version.sh` — never by hand.
