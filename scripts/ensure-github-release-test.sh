#!/usr/bin/env bash
# Test of scripts/ensure-github-release.sh as release.yml runs it: in a real Git
# repository with a real tag, with only `gh` doubled on PATH. It creates a
# missing release from the tag's own CHANGELOG.md section (not the working
# tree's), leaves an existing one untouched, accepts a create a concurrent one
# won, and refuses a missing tag, a gh refusal and a missing section by name.
# `just release-targets-check` (every `just check` tier) runs it.
#
# Quiet on success, one line. On failure it prints what the script said.
set -euo pipefail
cd "$(dirname "$0")/.." || {
  echo "ensure-github-release-test: cannot enter the repository root from $0; run it from a complete checkout" >&2
  exit 1
}
script="$PWD/scripts/ensure-github-release.sh"

work="$(mktemp -d)" || {
  echo "ensure-github-release-test: could not create a scratch directory; check that \$TMPDIR (or /tmp) is writable and has space" >&2
  exit 1
}
trap 'rm -rf "$work" || echo "ensure-github-release-test: could not remove $work; delete it by hand" >&2' EXIT

fail() {
  echo "ensure-github-release-test: $1" >&2
  if [ -s "$work/out" ]; then
    echo "  what the script said:" >&2
    cat "$work/out" >&2 || echo "  (unreadable: $work/out)" >&2
  fi
  echo "  fix scripts/ensure-github-release.sh (or this test, if the contract changed on purpose), then rerun just release-targets-check" >&2
  exit 1
}

# The forge double: every call is logged; GH_STATE says what the release
# lookup sees (absent, exists, race: absent until a create is attempted,
# refused: an auth failure) and GH_CREATE whether a create succeeds. It fails
# loudly (exit 70) if it cannot record a call, so none goes unchecked.
mkdir -p "$work/bin" || fail "could not create $work/bin; check that $work is writable"
cat >"$work/bin/gh" <<'EOF' || fail "could not write the gh double; check that $work is writable"
#!/usr/bin/env bash
set -euo pipefail
broken() { echo "gh double: could not $1" >&2; exit 70; }
printf '%s\n' "$*" >>"$GH_LOG" || broken "log the call to $GH_LOG"
case "$1 $2" in
  "release view")
    case "$GH_STATE" in
      exists) echo '{"tagName":"'"$3"'"}'; exit 0 ;;
      race) [ -e "$GH_LOG.created" ] && { echo '{"tagName":"'"$3"'"}'; exit 0; } ;;
      refused) echo 'HTTP 401: Bad credentials (https://api.github.com/graphql)' >&2; exit 1 ;;
    esac
    echo 'release not found' >&2
    exit 1
    ;;
  "release create")
    while [ "$#" -gt 0 ]; do
      if [ "$1" = --notes-file ]; then cp "$2" "$GH_LOG.notes" || broken "keep the notes file $2"; fi
      shift
    done
    if [ "$GH_STATE" = race ]; then
      : >"$GH_LOG.created" || broken "mark the concurrent create in $GH_LOG.created"
      echo 'HTTP 422: Validation Failed: tag_name already_exists' >&2
      exit 1
    fi
    [ "${GH_CREATE:-ok}" = ok ] || { echo 'HTTP 403: Resource not accessible by integration' >&2; exit 1; }
    exit 0
    ;;
esac
echo "gh double: unexpected call: $*" >&2
exit 1
EOF
chmod +x "$work/bin/gh" || fail "could not make the gh double executable; check that $work honors chmod"

# The repository: CHANGELOG.md as semantic-release's changelog step writes it,
# tagged v0.14.0, then changed in a later commit and again in the working tree.
repo="$work/repo"
tagged_section='## [0.14.0](https://github.com/nickderobertis/skilltest/compare/v0.13.0...v0.14.0) (2026-10-14)

### Bug Fixes

* **release:** publish both checksum names ([#88](https://github.com/nickderobertis/skilltest/issues/88))'
# A major release, whose heading semantic-release writes with one #.
major_section='# [1.0.0](https://github.com/nickderobertis/skilltest/compare/v0.14.0...v1.0.0) (2026-11-01)

### ⚠ BREAKING CHANGES

* go 1.0'
{
  git init -q "$repo" &&
    git -C "$repo" config user.email test@example.invalid &&
    git -C "$repo" config user.name "ensure-github-release-test" &&
    printf '# Changelog\n\n%s\n\n## [0.13.0](https://github.com/nickderobertis/skilltest/compare/v0.12.2...v0.13.0) (2026-10-13)\n\n### Features\n\n* an older change\n' "$tagged_section" >"$repo/CHANGELOG.md" &&
    # Older history well past a pipe buffer, so reading the top section must
    # not stop the changelog's reader early.
    for ((line = 0; line < 5000; line++)); do printf '* an older change with a long enough description to fill the pipe\n'; done >>"$repo/CHANGELOG.md" &&
    git -C "$repo" add CHANGELOG.md && git -C "$repo" commit -qm "chore(release): 0.14.0" &&
    git -C "$repo" tag v0.14.0 &&
    printf '# Changelog\n\n## [0.14.0] (rewritten later)\n\n* not what was released\n' >"$repo/CHANGELOG.md" &&
    git -C "$repo" commit -qam "docs: rewrite the changelog" &&
    printf '# Changelog\n\n## [0.14.0] (working tree)\n\n* uncommitted\n' >"$repo/CHANGELOG.md" &&
    git -C "$repo" tag v0.15.0 &&
    printf '# Changelog\n\n%s\n\n%s\n' "$major_section" "$tagged_section" >"$repo/CHANGELOG.md" &&
    git -C "$repo" commit -qam "chore(release): 1.0.0" &&
    git -C "$repo" tag v1.0.0 &&
    printf '# Changelog\n\n## [0.14.0] (working tree)\n\n* uncommitted\n' >"$repo/CHANGELOG.md"
} || fail "could not build the fixture repository; check that git is on PATH and $work is writable"

# $1 = GH_STATE, $2 = GH_CREATE, rest = the script's arguments.
ensure() {
  local state="$1" create="$2"
  shift 2
  { : >"$work/out" && : >"$work/gh.log" && rm -f "$work/gh.log.created" "$work/gh.log.notes"; } ||
    fail "could not reset $work; check that it is writable"
  (cd "$repo" && PATH="$work/bin:$PATH" GH_LOG="$work/gh.log" GH_STATE="$state" GH_CREATE="$create" bash "$script" "$@") >"$work/out" 2>&1
}

# (a) No release: one is created from the tag, titled with it, with the
# tagged section as its notes.
ensure absent ok v0.14.0 CHANGELOG.md || fail "the script failed to create a missing release"
grep -Eq '^release create v0\.14\.0 --verify-tag --title v0\.14\.0 --notes-file [^ ]+$' "$work/gh.log" ||
  fail "the script did not create the release from the existing tag (--verify-tag), titled with the tag; gh saw: $(cat "$work/gh.log")"
# Byte for byte: the committed section runs to the next version heading, so it
# ends with the blank line before it.
printf '%s\n\n' "$tagged_section" >"$work/expected" || fail "could not write $work/expected; check that $work is writable"
cmp -s "$work/expected" "$work/gh.log.notes" ||
  fail "the release notes are not, byte for byte, the v0.14.0 section of CHANGELOG.md as committed at the tag (heading through the blank line before the next version heading); diff: $(diff "$work/expected" "$work/gh.log.notes" | tr '\n' ' ')"
! grep -Eq '^release (edit|delete)' "$work/gh.log" || fail "the script edited or deleted a release while creating one"

# A major version's single-# heading ends at the next (## minor) heading.
ensure absent ok v1.0.0 CHANGELOG.md || fail "the script failed to create a missing major release"
printf '%s\n\n' "$major_section" >"$work/expected" || fail "could not write $work/expected; check that $work is writable"
cmp -s "$work/expected" "$work/gh.log.notes" ||
  fail "the v1.0.0 notes are not, byte for byte, its # [1.0.0] section up to the next version heading; diff: $(diff "$work/expected" "$work/gh.log.notes" | tr '\n' ' ')"

# (b) The release exists: nothing but the lookup reaches gh.
ensure exists ok v0.14.0 CHANGELOG.md || fail "the script failed when the release already exists"
[ "$(cat "$work/gh.log")" = "release view v0.14.0 --json tagName" ] ||
  fail "the script touched an existing release; it must leave it as it is, but gh saw: $(cat "$work/gh.log")"

# (c) A concurrent create won: the release is viewable afterwards, so proceed.
ensure race ok v0.14.0 CHANGELOG.md || fail "the script failed when a concurrent create won the race"
grep -q '^release create' "$work/gh.log" || fail "the race case never attempted a create"

# (d) Each refusal names its cause and exits non-zero.
expect_red() {
  local what="$1" says="$2"
  shift 2
  if ensure "$@"; then fail "the script succeeded where $what"; fi
  grep -Fq -- "$says" "$work/out" || fail "the script refused where $what without saying '$says'"
  ! grep -q '^release create' "$work/gh.log" || [ "$what" = "gh refuses the create" ] ||
    fail "the script attempted a create where $what"
}
expect_red "the tag is missing" "tag v0.16.0 is not in this clone" absent ok v0.16.0 CHANGELOG.md
expect_red "gh refuses the lookup for auth" "gh could not look up release v0.14.0 (HTTP 401: Bad credentials" refused ok v0.14.0 CHANGELOG.md
expect_red "gh refuses the create" "gh could not create release v0.14.0 (HTTP 403" absent refused v0.14.0 CHANGELOG.md
expect_red "the tagged changelog has no section for the version" "CHANGELOG.md at v0.15.0 has no section for 0.15.0" absent ok v0.15.0 CHANGELOG.md
expect_red "the changelog is not committed at the tag" "NEWS.md is not committed at v0.14.0" absent ok v0.14.0 NEWS.md
# The notes file is written under $TMPDIR; one that cannot hold it is named.
if TMPDIR="$work/absent" ensure absent ok v0.14.0 CHANGELOG.md; then fail "the script succeeded with no writable TMPDIR for its notes"; fi
grep -Fq "could not create a temporary file for the release notes" "$work/out" ||
  fail "the script did not name the release-notes file it could not create"
! grep -q '^release create' "$work/gh.log" || fail "the script attempted a create without its notes"
status=0
ensure absent ok v0.14.0 || status=$?
[ "$status" = 2 ] || fail "the script exited $status, not 2, without a changelog path"
grep -Fq "usage: ensure-github-release.sh" "$work/out" || fail "the script did not print its usage when called without a changelog path"
[ ! -s "$work/gh.log" ] || fail "the script called gh before refusing its invocation"

echo "ensure-github-release-test: creates a missing release from the tagged changelog section, leaves an existing one, accepts a lost race, and names each refusal"
