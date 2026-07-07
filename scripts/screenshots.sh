#!/usr/bin/env bash
# Capture the terminal screenshots that screencomp gates, galleries, and posts to
# PRs (see screencomp.toml + .github/workflows/visual-docs.yml).
#
# It drives the REAL release `skilltest` binary against the bundled deterministic
# `skilltest-fake-provider` (the `fake-provider` feature) using dedicated fixtures
# under screenshots/fixture/ — exactly as the e2e suite drives the built CLI — so
# the captured output is genuine CLI output; only the model is faked (no harness,
# no network, no cost). Each scene's output is rendered to a deterministic SVG by
# `freeze` using the VENDORED, pinned font
# (screenshots/fonts/JetBrainsMono-Regular.ttf), so the bytes — and therefore the
# screencomp digests — are identical on every machine and CI runner without a
# pinned container. That byte-determinism is the whole contract: change a
# command's output (or its formatting) and that scene's SVG (and hash) changes;
# otherwise it does not.
#
# Scenes — one per command, so the gallery documents the whole CLI surface:
#   run         the report, with a `format` toggle the gallery flips between the
#               two output formats: `human` (a directory run: PASS/FAIL summary
#               with the failing eval itemized) and `json` (the machine-readable
#               `--format json` report, the stable SDK contract).
#   mocks       a `run` of a case that mocks + spies on tool calls: a `stub`
#               intercepts `git push`, a `deny` blocks `rm -rf`, and the
#               deterministic `not_called` eval catches the blocked command.
#   validate    validating a folder of skills, one of which is missing its
#               `description` (a real finding).
#   init        scaffolding a starter project.
# skilltest's human output is plain text (no ANSI colour, no `--color` flag), so
# every scene is rendered the same way (`freeze --language ansi` on plain text).
#
# Output (screencomp's capture contract):
#   $SHOTS_OUT/captures.json   index: {schema, shots:[{name,toggles,hash,image}]}
#   $SHOTS_OUT/<scene>.svg     one SVG per scene (run has one per `format` toggle)
# $SHOTS_OUT defaults to shots/current/<arch> (the reusable workflow exports it
# per lane). The SVGs are also copied to docs/screenshots/ (committed) for the
# README + gallery.
#
# Requires `freeze` on PATH (install the pinned version with `just screenshots-tools`).
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

arch="$(uname -m)"
case "$arch" in
  x86_64 | amd64) arch="x86_64" ;;
  arm64 | aarch64) arch="arm64" ;;
esac
SHOTS_OUT="${SHOTS_OUT:-shots/current/$arch}"
font="$repo_root/screenshots/fonts/JetBrainsMono-Regular.ttf"
fixture="$repo_root/screenshots/fixture"
docs_dir="$repo_root/docs/screenshots"

if ! command -v freeze >/dev/null 2>&1; then
  echo "screenshots: 'freeze' not on PATH. Install the pinned version with:" >&2
  echo "             just screenshots-tools" >&2
  exit 1
fi

# Build the binaries the capture drives: the real CLI and the deterministic fake
# provider (the `fake-provider` feature builds both bins). Release, like a user
# would run.
skilltest_bin="$repo_root/target/release/skilltest"
fake_bin="$repo_root/target/release/skilltest-fake-provider"
if [ -z "${SCREENSHOTS_NO_BUILD:-}" ] || [ ! -x "$skilltest_bin" ] || [ ! -x "$fake_bin" ]; then
  cargo build --release --locked --features fake-provider -p skilltest-cli \
    --bin skilltest --bin skilltest-fake-provider >&2
fi

# Portable SHA-256 (Linux coreutils vs macOS/BSD).
sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

# Deterministic freeze flags. The vendored font (embedded into the SVG as base64)
# is what makes the output reproducible across machines; everything else is fixed
# window styling. Auto height follows the content, so it only moves when the
# captured text does.
freeze_flags=(
  # Force terminal/ANSI mode. freeze's content-based auto-detection is flaky —
  # it intermittently misreads output as a source file ("Language Unknown") and
  # then ignores --font.file and hangs fetching a default font over the network.
  # `--language ansi` is unconditional, offline, and byte-identical to the
  # auto-detected render; skilltest's output is plain text, so it renders the
  # text verbatim.
  --language ansi
  --font.file "$font"
  --font.family "JetBrains Mono"
  --font.size 14
  --window
  --background "#0d1117"
  --padding "20,30"
  --margin 0
  --border.radius 8
  # Fixed window width + line wrap so EVERY scene renders at the SAME pixel width.
  # The gallery and README display each SVG at one fixed width, so a per-scene
  # auto-width made the on-page text size wildly inconsistent — a narrow `init`
  # scaled up huge, a wide `json` shrank. A constant width keeps the rendered
  # text size uniform across cards; `--wrap` folds the few genuinely over-wide
  # lines (the itemized mock violation's observed-calls trail) at the same column
  # budget so nothing overflows the box. 92 columns clears the widest real scene
  # (the `json` report, 87 cols) with margin; 835px = 30+30 padding + 92*~8.42px/char.
  --width 835
  --wrap 92
)

rm -rf "$SHOTS_OUT"
mkdir -p "$SHOTS_OUT" "$docs_dir"
tmp_state="$(mktemp -d)"
trap 'rm -rf "$tmp_state"' EXIT

# captures.json identity is `name + JSON.stringify(toggles)`; entries collect one
# "name|toggles|hash|image" record per rendered scene, sorted at the end.
entries=()

# Render one captured text file to a scene SVG, hash it, and record it. Scenes
# only have to be non-empty (skilltest emits no ANSI, so there is nothing to
# require beyond output).
render_scene() {
  local name="$1" toggles="$2" image="$3" src="$4"
  if [ ! -s "$src" ]; then
    echo "screenshots: scene '$name' produced no output — cannot render." >&2
    exit 1
  fi
  # `< /dev/null`: freeze reads stdin whenever it is not a character device (its
  # IsPipe check), so under CI's piped stdin it would ignore the file argument and
  # render empty input ("No input"). Pointing stdin at /dev/null (a char device)
  # forces it down the read-the-file path on every runner.
  freeze "$src" "${freeze_flags[@]}" -o "$SHOTS_OUT/$image" </dev/null >&2
  local hash
  hash="$(sha256 "$SHOTS_OUT/$image")"
  entries+=("$name|$toggles|$hash|$image")
  # The committed copies: same bytes, just outside the gitignored shots/ tree.
  cp "$SHOTS_OUT/$image" "$docs_dir/$image"
}

# The fake provider stands in for the harness; `--platform`/`--model` are just
# labels the report echoes. Run from the fixture dir so the cases' relative
# `skill:` paths resolve and no absolute path leaks into the captured output.
run_cmd=(
  "$skilltest_bin" run
  --provider "$fake_bin" --platform demo --model fake
)

# --- run: the report, human (default) and json (the SDK contract) -------------
out="$tmp_state/run-human.txt"
( cd "$fixture" && "${run_cmd[@]}" cases ) >"$out" 2>/dev/null || true
render_scene "run" '{"format":"human"}' "run-human.svg" "$out"

out="$tmp_state/run-json.txt"
( cd "$fixture" && "${run_cmd[@]}" cases/greets_by_name.yaml --format json ) \
  >"$out" 2>/dev/null || true
render_scene "run" '{"format":"json"}' "run-json.svg" "$out"

# --- mocks: a stub + deny + spy run whose not_called eval catches a violation -
out="$tmp_state/mocks.txt"
( cd "$fixture" && "${run_cmd[@]}" mocks/guardrail.yaml ) >"$out" 2>/dev/null || true
render_scene "mocks" "{}" "mocks.svg" "$out"

# --- validate: a folder of skills, one missing its `description` --------------
# Findings print to stderr; run from the fixture dir so the finding's skill path
# is the stable relative `skills/broken`, not a per-checkout absolute path.
out="$tmp_state/validate.txt"
( cd "$fixture" && "$skilltest_bin" validate skills ) >/dev/null 2>"$out" || true
render_scene "validate" "{}" "validate.svg" "$out"

# --- init: scaffold a starter project (in a clean dir so the paths are stable) -
init_dir="$tmp_state/init"
mkdir -p "$init_dir"
out="$tmp_state/init.txt"
( cd "$init_dir" && "$skilltest_bin" init ) >"$out" 2>/dev/null || true
render_scene "init" "{}" "init.svg" "$out"

# Write captures.json, shots sorted by identity, schema 1, trailing newline — the
# exact shape screencomp's classify/manifest/gallery read. All fields are safe
# ASCII (names, toggle values, hex digests, file names), so plain printf is sound.
{
  printf '{\n  "schema": 1,\n  "shots": [\n'
  IFS=$'\n' sorted=($(printf '%s\n' "${entries[@]}" | sort)); unset IFS
  last=$((${#sorted[@]} - 1))
  for i in "${!sorted[@]}"; do
    IFS='|' read -r name toggles hash image <<<"${sorted[$i]}"
    comma=","
    [ "$i" -eq "$last" ] && comma=""
    printf '    {\n      "name": "%s",\n      "toggles": %s,\n      "hash": "%s",\n      "image": "%s"\n    }%s\n' \
      "$name" "$toggles" "$hash" "$image" "$comma"
  done
  printf '  ]\n}\n'
} >"$SHOTS_OUT/captures.json"

echo "screenshots: wrote ${#entries[@]} shots to $SHOTS_OUT and docs/screenshots/" >&2
