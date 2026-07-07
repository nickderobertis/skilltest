# Terminal screenshots

Deterministic SVG screenshots of skilltest's **real** CLI output, gated by
[screencomp](https://github.com/nickderobertis/screencomp). Informational —
**never part of `just check` or the CI gate**; the `Visual docs` workflow
(`.github/workflows/visual-docs.yml`) owns the comparison on PRs.

## What it is

`scripts/screenshots.sh` drives the **real release `skilltest` binary** against
the bundled deterministic `skilltest-fake-provider` (the `fake-provider` feature)
and the dedicated fixtures in `fixture/` — exactly as the e2e suite drives the
built CLI — so the captured text is genuine CLI output; only the model is faked
(no harness, no network, no cost). Each scene is rendered to an SVG by
[`freeze`](https://github.com/charmbracelet/freeze).

One shot per command, so the gallery documents the whole CLI surface:

- `run` — the report, with a `format` toggle the gallery flips between the two
  output formats:
  - `human` — the default report: a directory run of `fixture/cases/` (`skilltest
    run cases`), with two passing cases, one failing case whose failing eval is
    itemized, and the `N/M runs passed` summary.
  - `json` — the `--format json` machine report for a single case: the stable
    contract the SDKs are generated from (summary, per-run evals with their
    `detail`/`reason`, and the transcript).
- `mocks` — a `run` of `fixture/mocks/guardrail.yaml`, which `stub`s the skill's
  real `git push`, `deny`s any `rm -rf`, and asserts on the calls with the
  deterministic `called`/`not_called` evals. The deployer skill really attempts
  `rm -rf`, so `not_called` catches it — the scene shows a violated call
  expectation reported (with the observed calls), never a vacuous pass.
- `validate` — `skilltest validate skills` over `fixture/skills/`, one of which
  (`broken`) is missing its frontmatter `description`, so the scene shows a real
  `INVALID` finding + the `FAIL  N validation finding(s)` tally (captured from
  stderr, where findings print).
- `init` — `skilltest init` scaffolding a starter project (the `created …` lines
  and the next-step hints).

skilltest's human output is **plain text** (no ANSI colour, no `--color` flag),
so every scene renders the same way (`freeze --language ansi` on plain text). The
window styling (`freeze`'s theme) is what tints the rendered text; that is fixed
and deterministic.

**Consistent text size.** Every scene is rendered at a fixed window width
(`freeze --width 835`, with `--wrap 92` folding the few over-wide lines — the
itemized mock violation's observed-calls trail), so the gallery/README — which
display each SVG at one fixed width — render the text at the same size on every
card. Without this, auto-width made a narrow `init` scale up huge and a wide
`json` shrink.

**Path stability** (so the bytes/hashes are identical on every machine): the
`run` and `mocks` scenes are captured from within `fixture/`, so the cases'
relative `skill:` paths (and the `json` scene's `skill` field) stay relative
(`cases/../skills/scheduler`), never a per-checkout absolute path. `validate` is
captured from `fixture/` too, so the finding's skill path is the stable
`skills/broken`. The fake provider emits no timestamps, usage, or session ids, so
there is nothing else volatile to normalize.

## Why it is byte-reproducible (and needs no container)

screencomp gates on the **hash** of each image, so capture must be deterministic.
Unlike a rasterized PNG (whose anti-aliasing drifts across CPUs), an SVG is pure
layout math. We pin both inputs:

- **`freeze` is version-pinned** (`just`'s `freeze-version`, the CI
  `capture-command`, and `screenshots-tools` all agree on `0.2.2`).
- **The font is vendored** (`fonts/JetBrainsMono-Regular.ttf`, OFL — see
  `fonts/JetBrainsMono-OFL.txt`) and passed via `--font.file`, so freeze never
  fetches one over the network (which also makes capture offline and fast). The
  font is embedded into each SVG as base64, so the file renders the same on
  GitHub and crates.io with nothing external to load.

The result: identical bytes on every machine and runner, so a single `x86_64`
lane and baseline cover everyone — the SVG only changes when the output's
**content or formatting** changes, which is exactly what the gate should catch.

## Outputs

- `shots/current/<arch>/captures.json` + the SVGs — the capture screencomp reads
  (gitignored; regenerated). `$SHOTS_OUT` overrides the directory; the reusable
  workflow exports it per arch lane.
- `shots/baseline/<arch>.json` — the committed digest baseline (no images).
- `docs/screenshots/*.svg` — the committed copies embedded in the README.

## The animated demo GIF (`docs/screenshots/demo.gif`)

The SVGs are static; the README **hero** is an animated GIF of a typical run end
to end — each case resolving from queued to passed/failed as its evals return,
then clearing to reveal the genuine report. `scripts/demo-gif.py` drives the
**real release binary** against the same `fixture/cases/` for its data (genuine
cases, evals, and report), then reconstructs the frames and renders them with the
same **vendored JetBrains Mono font** — Pillow only, no `ttyd`/`ffmpeg`. Unlike
the SVGs it is **not** hash-gated (a GIF isn't byte-reproducible across Pillow
versions), so it is regenerated on demand (`just screenshots-gif`) and committed.
Regenerate it when the run/report format changes (`crates/skilltest-core/src/report.rs`).

## Commands

- `just screenshots-tools` — install the pinned `freeze` (needs Go). screencomp
  is installed separately (see its README); CI installs both itself.
- `just screenshots` — capture (builds the release binaries, writes the shots +
  the README copies). Quiet on success.
- `just screenshots-gif` — regenerate the animated demo GIF (needs Python 3 +
  Pillow). Builds the release binaries, then writes `docs/screenshots/demo.gif`.
- `just screenshots-bless` — after an **intended** output change, recapture and
  refresh `shots/baseline/<arch>.json`. Commit it alongside `docs/screenshots/`.

## The strict gate

CI (`fail-on-drift: true`) fails when a capture diverges from the committed
baseline. The local pre-push guard (`.githooks/pre-push`, enable with
`git config core.hooksPath .githooks`) re-captures **only** when a
`[guard].paths` file changes (`screencomp.toml`), and on drift it regenerates the
baseline, builds a review gallery (`shots/review/index.html`), and blocks the
push so you commit the refreshed baseline + README images deliberately.

## Changing the screenshots

Editing the report format (`crates/skilltest-core/src/report.rs`), the eval
detail summaries (`eval.rs`), the CLI surface (`cli.rs`), the scaffold text
(`scaffold.rs`), the fake provider (`fake_provider.rs`), the fixtures, or the
scenes in `scripts/screenshots.sh` will change the SVGs. That is expected — run
`just screenshots-bless` and commit the new baseline + `docs/screenshots/`.
Bumping `freeze-version` or the vendored font reflows every shot; bless once and
keep the three `freeze` version references in sync.
