#!/usr/bin/env bash
# Ensures the GitHub Release for a pushed tag exists before release.yml uploads
# archives to it, so the binaries never depend on semantic-release's own
# @semantic-release/github step having finished.
#
# Usage: ensure-github-release.sh <tag> <changelog-path>
#   Run from a clone holding <tag>; the repository is gh's (GH_REPO, or the
#   clone's remote).
#
# An existing release is left exactly as it is: no edit, delete or recreate.
# Otherwise one is created from the existing tag (--verify-tag, never a new
# tag), titled with the tag, whose notes are that version's section of
# <changelog-path> as committed at the tag: from its `## [X.Y.Z]` (or a major's
# `# [X.Y.Z]`) heading up to the next version heading. A create that loses a
# race to a concurrent one succeeds once the release is viewable.
#
# Quiet on success, one line. Exit codes: 0 the release exists; 1 the tag, its
# changelog section, or gh refused; 2 invalid invocation.
set -euo pipefail

err() {
  echo "ensure-github-release: $1" >&2
  exit 1
}

[ "$#" -eq 2 ] && [ -n "$1" ] && [ -n "$2" ] || {
  echo "ensure-github-release: usage: ensure-github-release.sh <tag> <changelog-path>; pass the release tag (vX.Y.Z) and the changelog's path in the repository" >&2
  exit 2
}
tag="$1" changelog="$2"
version="${tag#v}"

git rev-parse -q --verify "refs/tags/$tag^{commit}" >/dev/null ||
  err "tag $tag is not in this clone; fetch it (git fetch origin refs/tags/$tag:refs/tags/$tag) or check that semantic-release pushed it"

if viewed="$(gh release view "$tag" --json tagName 2>&1)"; then
  echo "ensure-github-release: release $tag already exists; left as it is"
  exit 0
fi
case "$viewed" in
  *"release not found"*) ;;
  *) err "gh could not look up release $tag ($viewed); check GH_TOKEN's access to the repository and rerun" ;;
esac

committed="$(git show "$tag:$changelog" 2>&1)" ||
  err "$changelog is not committed at $tag ($committed); check the changelog path and that semantic-release's changelog step committed it"
notes="$(awk -v version="$version" '
  function heading(line) { return line ~ /^##? \[?[0-9]+\.[0-9]+\.[0-9]+[^ ]*(\]| |\(|$)/ }
  function named(line,   v) {
    v = line
    sub(/^##? \[?/, "", v)
    sub(/[] (].*$/, "", v)
    return v == version
  }
  inside && heading($0) { exit }
  !inside && heading($0) && named($0) { inside = 1 }
  inside { print }
' <<<"$committed")"
[ -n "$notes" ] ||
  err "$changelog at $tag has no section for $version; check that semantic-release's changelog step wrote it, or create the release by hand"

body="$(mktemp)" || err "could not create a temporary file for the release notes; check that \$TMPDIR is writable"
trap 'rm -f "$body"' EXIT
printf '%s\n' "$notes" >"$body" || err "could not write the release notes to $body; check that \$TMPDIR has space"

if created="$(gh release create "$tag" --verify-tag --title "$tag" --notes-file "$body" 2>&1)"; then
  echo "ensure-github-release: created release $tag from its changelog section"
  exit 0
fi
# A concurrent create (semantic-release's own step, or a re-run) may have won.
if gh release view "$tag" --json tagName >/dev/null 2>&1; then
  echo "ensure-github-release: release $tag was created concurrently; left as it is"
  exit 0
fi
err "gh could not create release $tag ($created); check GH_TOKEN's contents:write access and that $tag is on the remote"
