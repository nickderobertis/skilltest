#!/usr/bin/env bash
#
# Install a prebuilt `oneharness` from its GitHub Release, verifying the checksum.
#
# skilltest's live e2e drives a real harness *through* oneharness, so both CI and
# a local run need the binary on PATH. Building from source is slow; this grabs
# the release artifact for the host target instead and checks its `.sha256` before
# trusting it (the same contract scripts/install.sh uses for skilltest itself).
#
# Usage:
#   scripts/install-oneharness.sh [version]      # default: the pinned version below
# Env:
#   ONEHARNESS_INSTALL_DIR   where to put the binary (default: ~/.local/bin)
#
# Requires `gh` (authenticated) for the download — present and authed on GitHub
# Actions runners, and the repo's normal local tool. Quiet on success: one line.

set -euo pipefail

# Pinned to the version skilltest's OneharnessProvider targets, and kept in
# lockstep with the `oneharness-cli` bounds both SDKs declare and the
# `install-oneharness` recipe's default in the justfile. The whole argv surface
# skilltest drives landed by v0.3.8 — `--system` (v0.2.1, the skill as a real
# system prompt on every harness), native reply extraction (v0.2.37, OpenCode's
# nested JSONL included), `--events`/`--stream` (v0.3.6), the mock/spy seam
# (v0.3.7: `oneharness mock`, `run --mock-rules`/`--spy-file`) and opt-in run
# history (v0.3.8: `run --history --history-dir --history-name`, the report's
# `history_file`, and the `oneharness history` verbs).
#
# v0.16.0 is what skilltest targets now. Two things it changes matter here:
# `run` prints a human-readable report unless `--format json` or `--compact` is
# given (skilltest always passes `--compact`, or `--stream`, whose NDJSON is
# format-independent), and `run_mode` is a config key the whole 0.16 line
# parses — so the repo-root `oneharness.toml` carries it directly instead of the
# `ONEHARNESS_RUN_MODE` env workaround the recipes used to layer on. It also
# reports `supports_resume` for every registered harness, which is why
# `skilltest_core::provider::supports_resume` now mirrors the whole registry
# (the `oneharness list` drift alarm in tests/oneharness_integration.rs).
#
# Note v0.3.0 normalized `--mode` approval modes (breaking): skilltest passes no
# `--mode`, so oneharness's default applies — configure approval (e.g. `bypass`)
# via oneharness's own config. History written by a pre-0.16 skilltest lives in
# the legacy store format; `oneharness history migrate` rewrites it in place.
# Bump here when skilltest adopts a newer oneharness.
default_version="v0.16.0"
version="${1:-$default_version}"
repo="nickderobertis/oneharness"
dest="${ONEHARNESS_INSTALL_DIR:-$HOME/.local/bin}"

fail() { printf 'install-oneharness: %s\n' "$1" >&2; exit 1; }

command -v gh >/dev/null 2>&1 \
    || fail "needs the GitHub CLI (\`gh\`). Install it, or build oneharness from source: cargo install --git https://github.com/$repo --tag $version --locked"

os="$(uname -s)"; arch="$(uname -m)"
case "$os/$arch" in
    Darwin/arm64)          target="aarch64-apple-darwin" ;;
    Darwin/x86_64)         target="x86_64-apple-darwin" ;;
    Linux/x86_64)          target="x86_64-unknown-linux-gnu" ;;
    Linux/aarch64|Linux/arm64) target="aarch64-unknown-linux-gnu" ;;
    *) fail "unsupported host $os/$arch — build from source: cargo install --git https://github.com/$repo --tag $version --locked" ;;
esac

asset="oneharness-$version-$target"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

gh release download "$version" --repo "$repo" \
    --pattern "$asset.tar.gz" --pattern "$asset.sha256" \
    --dir "$tmp" --clobber \
    || fail "could not download $asset from $repo@$version (is the release published and gh authenticated?)"

want="$(awk '{print $1}' "$tmp/$asset.sha256")"
if command -v sha256sum >/dev/null 2>&1; then
    got="$(sha256sum "$tmp/$asset.tar.gz" | awk '{print $1}')"
else
    got="$(shasum -a 256 "$tmp/$asset.tar.gz" | awk '{print $1}')"
fi
[ -n "$want" ] && [ "$want" = "$got" ] \
    || fail "checksum mismatch for $asset.tar.gz (want=$want got=$got) — refusing to install"

tar -xzf "$tmp/$asset.tar.gz" -C "$tmp"
mkdir -p "$dest"
install -m 0755 "$tmp/oneharness" "$dest/oneharness"

printf 'oneharness %s installed to %s\n' "$version" "$dest/oneharness"
