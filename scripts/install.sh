#!/bin/sh
# Install the `skilltest` binary from GitHub Releases.
#
#   curl -fsSL https://raw.githubusercontent.com/nickderobertis/skilltest/main/scripts/install.sh | sh
#
# Environment:
#   SKILLTEST_VERSION      tag to install (default: the latest release)
#   SKILLTEST_INSTALL_DIR  install directory (default: $HOME/.local/bin)
#   SKILLTEST_DRY_RUN      if set, print the resolved target/URL and exit
#
# Detects your OS/arch, downloads the matching archive, verifies its sha256
# checksum, and installs the binary. Supports Linux and macOS on x86_64/aarch64.

set -eu

REPO="nickderobertis/skilltest"
BIN="skilltest"
INSTALL_DIR="${SKILLTEST_INSTALL_DIR:-$HOME/.local/bin}"

err() {
	printf 'install.sh: %s\n' "$1" >&2
	exit 1
}

# Print a command's output, preferring curl, falling back to wget.
fetch() {
	if command -v curl >/dev/null 2>&1; then
		curl -fsSL "$1"
	elif command -v wget >/dev/null 2>&1; then
		wget -qO- "$1"
	else
		err "need curl or wget to download files"
	fi
}

# Download a URL to a file.
download() {
	if command -v curl >/dev/null 2>&1; then
		curl -fsSL -o "$2" "$1"
	else
		wget -qO "$2" "$1"
	fi
}

detect_target() {
	os="$(uname -s)"
	arch="$(uname -m)"
	case "$os" in
	Linux) os_part="unknown-linux-gnu" ;;
	Darwin) os_part="apple-darwin" ;;
	*) err "unsupported OS: $os (supported: Linux, macOS)" ;;
	esac
	case "$arch" in
	x86_64 | amd64) arch_part="x86_64" ;;
	arm64 | aarch64) arch_part="aarch64" ;;
	*) err "unsupported architecture: $arch (supported: x86_64, aarch64)" ;;
	esac
	printf '%s-%s' "$arch_part" "$os_part"
}

sha256_of() {
	if command -v sha256sum >/dev/null 2>&1; then
		sha256sum "$1" | awk '{print $1}'
	elif command -v shasum >/dev/null 2>&1; then
		shasum -a 256 "$1" | awk '{print $1}'
	else
		err "need sha256sum or shasum to verify the download"
	fi
}

resolve_version() {
	if [ -n "${SKILLTEST_VERSION:-}" ]; then
		printf '%s' "$SKILLTEST_VERSION"
		return
	fi
	# Parse tag_name from the GitHub API without requiring jq.
	tag="$(fetch "https://api.github.com/repos/$REPO/releases/latest" |
		grep '"tag_name"' | head -1 |
		sed -E 's/.*"tag_name": *"([^"]+)".*/\1/')"
	[ -n "$tag" ] || err "could not determine the latest release; set SKILLTEST_VERSION"
	printf '%s' "$tag"
}

main() {
	target="$(detect_target)"
	version="$(resolve_version)"
	asset="$BIN-$target.tar.gz"
	base="https://github.com/$REPO/releases/download/$version"
	url="$base/$asset"

	if [ -n "${SKILLTEST_DRY_RUN:-}" ]; then
		printf 'target:  %s\nversion: %s\nurl:     %s\n' "$target" "$version" "$url"
		exit 0
	fi

	tmp="$(mktemp -d)"
	trap 'rm -rf "$tmp"' EXIT

	download "$url" "$tmp/$asset" || err "failed to download $url"
	download "$url.sha256" "$tmp/$asset.sha256" ||
		err "failed to download checksum $url.sha256"

	expected="$(awk '{print $1}' "$tmp/$asset.sha256")"
	actual="$(sha256_of "$tmp/$asset")"
	[ "$expected" = "$actual" ] ||
		err "checksum mismatch for $asset (expected $expected, got $actual)"

	tar -xzf "$tmp/$asset" -C "$tmp"
	[ -f "$tmp/$BIN" ] || err "archive did not contain a '$BIN' binary"

	mkdir -p "$INSTALL_DIR"
	install -m 0755 "$tmp/$BIN" "$INSTALL_DIR/$BIN" 2>/dev/null ||
		{ cp "$tmp/$BIN" "$INSTALL_DIR/$BIN" && chmod 0755 "$INSTALL_DIR/$BIN"; }

	printf 'installed %s %s to %s/%s\n' "$BIN" "$version" "$INSTALL_DIR" "$BIN"
	case ":$PATH:" in
	*":$INSTALL_DIR:"*) ;;
	*) printf 'note: %s is not on your PATH; add it to use `%s` directly.\n' "$INSTALL_DIR" "$BIN" ;;
	esac
}

main "$@"
