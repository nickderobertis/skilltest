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

# The targeted oneharness release, and the one place it is authored. The
# `just install-oneharness` default and both SDKs' `oneharness-cli` bounds
# restate it; `oneharness_pin_is_lockstep_across_installer_recipe_and_both_sdks`
# (crates/skilltest-cli/tests/e2e.rs) reconciles all four, so bump it here and
# let that test name whatever else has to move. What this line has to satisfy
# lives in docs/protocol.md (the argv and report surface the provider drives).
#
# Upgrading past a release that changed the history store: `oneharness history
# migrate` rewrites an older store in place — skilltest never does it for you.
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
