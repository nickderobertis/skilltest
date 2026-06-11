# AGENTS.md

Durable instructions for humans and agents working in this repo. Write for a
future maintainer, not as a session log. Put deterministic steps in scripts and
keep this file for constraints, tradeoffs, and judgment.

> `CLAUDE.md` is a symlink to this file (`ln -s AGENTS.md CLAUDE.md`) so the two
> never drift. Edit `AGENTS.md` only.

## What this repo is

`skilltest` is a framework for testing AI **skills** (a `SKILL.md` plus its
assets). The artifact is a **Rust CLI** (`skilltest`) plus thin **plugins** that
expose the same capability inside existing test runners — `pytest` (Python) and
`vitest` (TypeScript) to start. It runs a skill on one or more harness/model
**platforms** via [`oneharness`](https://github.com/nickderobertis/oneharness),
optionally driving a simulated user across multiple turns, then scores the
transcript with built-in **natural-language evals** (boolean and numeric). It
also validates skill definitions.

Consumers: skill authors who want a regression suite for a skill, and CI that
must prove a skill still behaves.

## Layout

| Path | What |
| --- | --- |
| `crates/skilltest-core` | Library: config, skill model + validation, test-case model, provider protocol, evals, runner, report. The stable Rust API the plugins and CLI build on. |
| `crates/skilltest-cli` | The `skilltest` binary (clap). Also ships `skilltest-fake-provider`, a deterministic reference provider used by the e2e suite. |
| `plugins/pytest` | `skilltest-pytest`: Python API + pytest collection, wrapping the CLI's JSON contract. |
| `plugins/vitest` | `@skilltest/vitest`: TypeScript API + vitest helpers, wrapping the same JSON contract. |
| `tests/fixtures` | Sample skills and YAML test cases shared by the e2e suites. |
| `docs/` | The provider protocol, config/test-case schema, and live-e2e (`docs/e2e.md`) references. |
| `scripts/install.sh` | Installs a prebuilt `skilltest` from a GitHub Release (verifies checksum). |
| `scripts/install-oneharness.sh` | Installs the prebuilt `oneharness` the live e2e drives (verifies checksum). |
| `scripts/e2e-lib.sh`, `scripts/e2e-harness.sh` | Live, per-harness e2e: drive the built CLI against a *real* harness through oneharness. See `docs/e2e.md`. |
| `gh-secrets.json` | Declarative secret manifest, synced from Bitwarden to the GitHub repo + a gitignored local `.env` via `gh-secrets manifest sync`. |
| `.github/workflows/release.yml` | Tag-triggered cross-platform binary build + checksums. |
| `.github/workflows/e2e-claude.yml` | Live claude-code e2e, gated to the canonical repo and non-fork PRs. |

## Command surface

Use the `just` recipes; do not hand-roll equivalent commands.

- `just bootstrap` — set up from a clean clone (cargo fetch + `uv sync` + `pnpm install`).
- `just check` — full quality gate (format, lint, type check, unit + e2e tests).
  Must pass before any commit or PR.
- `just test` / `just lint` / `just format` / `just typecheck` — individual gate steps.
- `just test-e2e` — the cross-language end-to-end suites (Rust + both plugins).
- `just upgrade` — upgrade dependencies across all three stacks, then re-run `just check`.
- `just install-oneharness` / `just test-live` / `just test-harness <id>` — the
  **opt-in live e2e** against a real harness (never in `just check`; needs
  `oneharness`, a harness binary, a synced secret, and network). See `docs/e2e.md`.

`just` needs `cargo`, `uv`, and `node`/`pnpm` on `PATH`. CI installs all three;
locally, install them once (see `docs/development.md`).

## The provider boundary

`skilltest` never talks to a model directly. The `Provider` trait
(`provider.rs`) has two real backends; see [`docs/protocol.md`](docs/protocol.md).

- **`OneharnessProvider` (default).** Targets
  [`oneharness`](https://github.com/nickderobertis/oneharness) **v0.2.0+** and
  uses four of its normalized features directly so skilltest can stop string-
  munging: `--system <skill instructions>` carries the skill as a real system
  prompt; `--resume <session_id>` continues a real harness session for the
  multi-turn loop on harnesses where `supports_resume` is true (claude-code,
  opencode, cursor today — others fall back to inlining the transcript);
  `results[*].usage` is aggregated into the report (`{input_tokens,
  output_tokens, cost_usd}`); and `results[*].failure_kind` (`auth` /
  `rate_limit` / `model_not_found` / `quota`) is surfaced through `Error::Provider
  { kind }` so the CLI gives a pointed hint. Evals and the simulated user run
  on a fixed `judge_harness`, independent of the harness under test. Verdict
  JSON is parsed tolerantly (real models wrap it in prose/fences) and
  type-checked.
- **`CommandProvider`.** A small JSON-lines protocol (one request object on
  stdin, one response on stdout, per op) backing the bundled
  `skilltest-fake-provider` and any custom provider. Custom providers may
  optionally emit `usage` and `session_id` on `respond` to participate in cost
  reporting and stateful multi-turn.

The fake provider is why the whole pipeline is testable without a live model: it
implements the protocol deterministically, so the default e2e suites exercise the
real argument parsing, YAML loading, conversation loop, eval logic, exit codes,
and JSON output — everything except the non-deterministic model. The
`OneharnessProvider` path is proven separately by the opt-in live tests
(`crates/skilltest-cli/tests/live.rs`, the deep claude-code suite) plus the
generic per-harness smoke (`scripts/e2e-harness.sh`), which run against real
oneharness + a real harness and are never in the gate. Caveat worth knowing:
skilltest carries the skill via `--system`, and **oneharness v0.2.0 only maps
that to a real system prompt for claude-code** — other harnesses receive it as a
positional arg they reject, so claude-code is the only live-green harness today.
`docs/e2e.md` holds the full matrix, the secrets flow (`gh-secrets.json`), and
the runbook for adding a harness.

## Invariants (non-negotiable)

- The quality gate is strict: no warnings-only mode. `clippy`, `ruff`, `ty`,
  `biome`, and `tsc` all fail the build on findings. A diagnostic is either an
  error or suppressed with a documented, tracked rationale.
- Validate all external / IO inputs at trust boundaries: config files, test-case
  YAML, skill frontmatter, and every provider response are parsed into typed
  models (`serde` in Rust, Pydantic in Python, Zod in TS) before use. Never trust
  raw provider output.
- The CLI's `--format json` output is a **stable contract** the plugins depend
  on. Changing its shape is a breaking change; update the Rust types, the
  Pydantic models, and the Zod schema together, and bump versions.
- Keep the artifact portable across the supported platform matrix (Linux, macOS).
- Do not commit secrets, credentials, PII, or customer data. Real provider runs
  need API keys; those live in the environment, never in fixtures or config.
- No non-determinism in the gate: the LLM is always faked in tests.

## Scripts and output are context

- Every script you add should be quiet on success — a single line or nothing.
- On failure, print the exact error and a concrete suggested next action.
- The CLI follows the same rule: minimal human output on success, the exact
  problem plus a suggested action on stderr, distinct exit codes (see
  `crates/skilltest-core/src/exit.rs`).

## Tests are context engineering

- Tests are how you and future agents actually see this system behave, so invest
  in them deliberately.
- The e2e suites drive the **built** CLI the way users do — as a subprocess,
  asserting on exit codes and JSON — against the fake provider. When you touch
  the conversation loop, evals, or the JSON contract, extend an e2e journey
  rather than adding another narrow unit test.
- Every e2e suite must cover at least one happy path **and** one meaningful
  failure/recovery path (a failing eval, a malformed config, a missing provider).

## Keeping the allowlist current

- The agent command allowlist lives in `.claude/settings.json`; the tool
  enforces it, so this file does not restate "follow the allowlist."
- Your job is to keep it current: when a new routine command becomes part of the
  normal build/test/release workflow, add it to the allowlist instead of
  re-approving it every session. Keep it narrow.

## Conventions

- Rust: stable toolchain, `rustfmt` defaults, `clippy -D warnings`. Errors use
  `thiserror`; the boundary between library errors and process exit codes lives
  in the CLI, not the core.
- Python plugin: Python 3.12+, `uv`, `ruff`, `ty`, `pytest`. Public API is
  re-exported from `skilltest_pytest/__init__.py`; everything else is internal.
- TS plugin: `strict` TypeScript, `biome` for lint+format, `vitest`, `pnpm`.
  Public API is the package entry in `src/index.ts`.
- See `tests/AGENTS.md` for test-fixture conventions.

## After the main task: refine and hand off

After completing a requested task, propose only materially-helpful follow-ups
(scripts to automate a manual step, a constraint worth recording here, a fixture
that improves visibility). Skip busywork. If nothing is materially helpful, say
so and stop.
