#!/usr/bin/env bash
# llmlint: ignore-file[new_code_lands_in_a_project] repo-level release gate over release-targets.toml and .github/workflows/publish.yml, neither of which belongs to an Nx package; run workspace-wide from `just check` like scripts/gen-contract.sh --check (AGENTS.md: scripts/*.sh are orchestrator-independent glue).
# Drift gate for release-targets.toml: what it declares against what this
# repository really publishes.
#
# A consumer sequencing work across repositories reads release-targets.toml to
# learn which skilltest artifact to wait on, and a target nobody declared grants
# that consumer no hold at all — silently. So the published set is DERIVED here
# from the release configuration rather than transcribed:
#
#   crate — each `publish_crate <name>` call in publish.yml's crates job, with
#           the crates/*/Cargo.toml whose [package] name it is.
#   pypi  — each `pyproject_version <dir>` call in its pypi job, named by that
#           dir's pyproject.toml [project] name and cross-checked against the
#           `on_pypi <name>` skip guard beside it.
#   npm   — each `publish_pkg <dir>` call in its npm job, named by that dir's
#           package.json; plus the per-platform packages its `for target in`
#           loop stages, mapped to a directory by scripts/stage-npm-binary.sh.
#
# A per-platform package is a `covers` entry of the target whose release ships
# it, never a target of its own, and it must also be pinned in that target's
# optionalDependencies — the covers list is what a consumer reading the
# declaration sees, the pin is what makes an install resolve the binary.
#
# Fails in both directions — a published name neither declared nor covered, and
# a declared or covered name nothing publishes — naming each drift and its fix.
# The schema itself is `onevcs release declaration`'s to enforce, so this reads
# only the fields the comparison needs — and refuses those when they are not the
# one quoted value (or list of them) the comparison would otherwise misread.
#
# Quiet on success, one line.
set -euo pipefail
cd "$(dirname "$0")/.." || {
  echo "check-release-targets: cannot enter the repository root from $0; run it from a complete checkout" >&2
  exit 1
}

declarations="release-targets.toml"
workflow=".github/workflows/publish.yml"
stager="scripts/stage-npm-binary.sh"
DECLARATION_SCHEMA_VERSION=3

fails=0
fail() {
  printf 'release-target drift: %s\n' "$1" >&2
  fails=$((fails + 1))
}

# The first `name = "..."` inside a TOML section, or empty when the file cannot
# be read — every caller fails on empty, naming the manifest. $1 = file, $2 = section.
toml_section_name() {
  awk -v section="[$2]" '
    $0 == section { inside = 1; next }
    inside && /^\[/ { exit }
    inside && /^name *= *"[^"]+"/ {
      sub(/^name *= *"/, ""); sub(/".*$/, ""); print; exit
    }
  ' "$1" 2>/dev/null || true
}

# Empty unless the file is JSON whose name is a non-empty string; every caller
# fails on empty, naming the manifest.
json_name() {
  jq -r 'if (.name | type) == "string" then .name else "" end' "$1" 2>/dev/null || true
}

# The jobs of publish.yml, one body each. $1 = job id.
job_body() {
  awk -v job="  $1:" '
    $0 == job { inside = 1; next }
    inside && /^  [A-Za-z0-9_-]+:$/ { exit }
    inside { print }
  ' "$workflow" || {
    echo "check-release-targets: could not read $workflow; check its permissions" >&2
    exit 1
  }
}

for required in "$declarations" "$workflow" "$stager"; do
  [ -f "$required" ] || {
    echo "check-release-targets: $required is missing; restore it from git — this gate derives the published set from it" >&2
    exit 1
  }
done
command -v jq >/dev/null 2>&1 || {
  echo "check-release-targets: jq is required to read the npm manifests; install jq and retry" >&2
  exit 1
}

if [ ! -r "$declarations" ]; then
  echo "check-release-targets: cannot read $declarations; check its permissions" >&2
  exit 1
fi

# llmlint: ignore-block[boundary_inputs_validated] This reads release-targets.toml only for the fields the drift comparison needs and refuses those when malformed; validating the whole document is the authoritative reader's job — `onevcs release declaration`, which reads it on every consumption — and onevcs is this repository's consumer rather than a dependency, so the gate does not take on a consumer's tool to re-check what that consumer validates whenever it reads the file.
# One record per field this gate reads: <entry>\t<key>\t<value>, entry 0 the
# top level, a list one record per element, and `!` a refusal.
fields="$(awk '
  function refuse(m) { printf "!\t%d\t%s\n", NR, m }
  function emit(k, v) {
    if (seen[entry, k]++) { refuse("writes " k " twice in one entry; keep the one that is right"); return }
    printf "%s\t%s\t%s\n", entry, k, v
  }
  BEGIN { entry = 0; reading = 1 }
  { line = $0; sub(/[ \t\r]+$/, "", line) }
  in_list {
    if (line ~ /^\]$/) { in_list = 0; next }
    if (line ~ /^[ \t]*"[^"]+",?$/) {
      if (open_element) { refuse("writes a covers entry with no comma before the next one; end every entry but the last with ,"); next }
      open_element = (line !~ /,$/)
      v = line; sub(/^[ \t]*"/, "", v); sub(/",?$/, "", v)
      printf "%s\t%s\t%s\n", entry, "covers", v; next
    }
    refuse("writes covers with a line that is not one quoted id: " line); next
  }
  line == "[[target]]" { entry = ++targets; reading = 1; next }
  line ~ /^\[/ { reading = 0; next }
  !reading { next }
  entry == 0 && match(line, /^schema_version = /) { emit("schema_version", substr(line, RLENGTH + 1)); next }
  entry == 0 && match(line, /^probe = /) { key = "probe" }
  entry > 0 && match(line, /^(id|name|manifest) = /) { key = substr(line, 1, RLENGTH - 3) }
  entry > 0 && line ~ /^covers = / {
    if (seen[entry, "covers"]++) { refuse("writes covers twice in one entry; merge them into one list"); next }
    if (line == "covers = [") { in_list = 1; open_element = 0; next }
    if (line !~ /^covers = \[("[^"]+"(, *"[^"]+")*)?\]$/) { refuse("writes covers as something other than a list of quoted ids: " line); next }
    body = line
    while (match(body, /"[^"]+"/)) {
      printf "%s\t%s\t%s\n", entry, "covers", substr(body, RSTART + 1, RLENGTH - 2)
      body = substr(body, RSTART + RLENGTH)
    }
    next
  }
  key != "" {
    rest = line; sub(/^[a-z_]+ = /, "", rest)
    if (rest ~ /^"[^"]+"$/) emit(key, substr(rest, 2, length(rest) - 2))
    else refuse("writes " key " as something other than one non-empty quoted string: " line)
    key = ""
  }
  END {
    if (in_list) refuse("leaves covers open; close its list with ]")
    printf "#\ttargets\t%d\n", targets + 0
  }
' "$declarations")" || {
  echo "check-release-targets: could not read $declarations; check its permissions" >&2
  exit 1
}
# llmlint: ignore-end[boundary_inputs_validated]

# $1 = entry, $2 = key. Every value it holds, one per line.
values_of() {
  printf '%s\n' "$fields" | awk -F'\t' -v e="$1" -v k="$2" '$1 == e && $2 == k { print $3 }'
}

while IFS=$'\t' read -r entry line message; do
  [ "$entry" = "!" ] && fail "$declarations line $line $message"
done < <(printf '%s\n' "$fields")

version="$(values_of 0 schema_version)"
[ "$version" = "$DECLARATION_SCHEMA_VERSION" ] ||
  fail "$declarations declares schema_version '$version' and this gate reads $DECLARATION_SCHEMA_VERSION; write schema_version = $DECLARATION_SCHEMA_VERSION, or bring this gate and scripts/release-probe.sh up to the new version together"
probe="$(values_of 0 probe)"
[ "$probe" = "scripts/release-probe.sh" ] && [ -x "$probe" ] ||
  fail "$declarations names probe '$probe'; write probe = \"scripts/release-probe.sh\" and keep that script committed executable (git update-index --chmod=+x)"

target_count="$(values_of '#' targets)"
declared="" # one "<id>\t<name>\t<manifest>" per target, in declaration order
covered=""  # one "<id>\t<covering target id>" per covers entry
entry=1
while [ "$entry" -le "$target_count" ]; do
  id="$(values_of "$entry" id)"
  name="$(values_of "$entry" name)"
  manifest="$(values_of "$entry" manifest)"
  [ -n "$id" ] && [ -n "$name" ] && [ -n "$manifest" ] ||
    fail "$declarations [[target]] $entry ('$id') lacks an id, name or manifest; give it all three, since the manifest is what pins its name to a real package"
  declared="${declared}${id}	${name}	${manifest}
"
  covers="$(values_of "$entry" covers)"
  while read -r c; do
    [ -n "$c" ] && covered="${covered}${c}	${id}
"
  done <<<"$covers"
  entry=$((entry + 1))
done
declared_ids="$(printf '%s' "$declared" | cut -f1)"
covered_ids="$(printf '%s' "$covered" | cut -f1)"

while read -r dup; do
  [ -n "$dup" ] && fail "$declarations declares or covers '$dup' more than once; an artifact is one target or one covers entry, so drop the extra"
done < <(printf '%s\n%s\n' "$declared_ids" "$covered_ids" | sed '/^$/d' | sort | uniq -d)
while read -r dup; do
  [ -n "$dup" ] && fail "$declarations gives the short name '$dup' to more than one target; a consumer's plan selects a target by that name, so give each target its own"
done < <(printf '%s' "$declared" | cut -f2 | sort | uniq -d)

published="" # one "<id>\t<manifest>" per target-shaped artifact, in publish order
platforms="" # one "<id>\t<manifest>" per per-platform npm package

crates_job="$(job_body crates)"
while read -r crate; do
  [ -n "$crate" ] || continue
  manifest=""
  for candidate in crates/*/Cargo.toml; do
    [ "$(toml_section_name "$candidate" package)" = "$crate" ] && manifest="$candidate"
  done
  if [ -z "$manifest" ]; then
    fail "$workflow publishes crate '$crate' but no crates/*/Cargo.toml has that [package] name; fix the publish_crate call or the manifest"
    continue
  fi
  published="${published}crate:${crate}	${manifest}
"
done < <(printf '%s\n' "$crates_job" | sed -n 's/^ *publish_crate \([A-Za-z0-9_-]*\) *$/\1/p')

pypi_job="$(job_body pypi)"
pypi_guards="$(printf '%s\n' "$pypi_job" | sed -n 's/.*on_pypi \([A-Za-z0-9._-]*\) ".*/\1/p')"
while read -r dir; do
  [ -n "$dir" ] || continue
  manifest="$dir/pyproject.toml"
  name="$( [ -f "$manifest" ] && toml_section_name "$manifest" project || true)"
  if [ -z "$name" ]; then
    fail "$workflow publishes the Python project in '$dir' but $manifest has no [project] name; restore it or fix the pyproject_version call"
    continue
  fi
  printf '%s\n' "$pypi_guards" | grep -Fxq -- "$name" ||
    fail "$workflow publishes $manifest ('$name') but its on_pypi skip guard names a different project; make the on_pypi call name '$name'"
  published="${published}pypi:${name}	${manifest}
"
done < <(printf '%s\n' "$pypi_job" | sed -n 's/.*pyproject_version \([^)]*\)).*/\1/p')

npm_job="$(job_body npm)"
printf '%s\n' "$npm_job" | grep -Fq "bash $stager" ||
  fail "$workflow's npm job no longer stages platform packages through $stager, so the platform names below are derived from a script nothing runs; point this gate at whatever stages them now"
targets="$(printf '%s\n' "$npm_job" | awk '
  /for target in / { inside = 1; sub(/.*for target in /, "") }
  inside { line = $0; done = sub(/; *do.*$/, "", line); gsub(/\\/, "", line); print line; if (done) exit }
' | tr ' ' '\n' | sed '/^$/d')"
[ -n "$targets" ] ||
  fail "$workflow's npm job has no 'for target in ...' loop over the platform packages; point this gate's target extraction at its new shape"
while read -r target; do
  [ -n "$target" ] || continue
  pkg="$(sed -n "s/^${target}) pkg=\"\\([^\"]*\\)\".*/\\1/p" "$stager")" || {
    echo "check-release-targets: could not read $stager; check its permissions" >&2
    exit 1
  }
  manifest="sdks/typescript/platforms/$pkg/package.json"
  if [ -z "$pkg" ] || [ ! -f "$manifest" ]; then
    fail "$workflow stages target '$target' but $stager maps it to no committed platform package; add its case arm and sdks/typescript/platforms/<pkg>/package.json"
    continue
  fi
  name="$(json_name "$manifest")"
  if [ -z "$name" ]; then
    fail "$manifest is not JSON with a string \"name\", so the platform package it publishes cannot be named; restore its name"
    continue
  fi
  platforms="${platforms}npm:${name}	${manifest}
"
done < <(printf '%s\n' "$targets")
for manifest in sdks/typescript/platforms/*/package.json; do
  [ -f "$manifest" ] || continue
  printf '%s' "$platforms" | cut -f2 | grep -Fxq -- "$manifest" ||
    fail "$manifest is committed but $workflow's npm job never stages it, so it is never published; add its rust target to that job's 'for target in' loop, or delete the package"
done
while read -r dir; do
  [ -n "$dir" ] || continue
  manifest="$dir/package.json"
  name="$( [ -f "$manifest" ] && json_name "$manifest" || true)"
  if [ -z "$name" ]; then
    fail "$workflow publishes the npm package in '$dir' but $manifest is missing or has no string name; restore it or fix the publish_pkg call"
    continue
  fi
  published="${published}npm:${name}	${manifest}
"
done < <(printf '%s\n' "$npm_job" | sed -n 's/^ *publish_pkg \([^ ]*\).*$/\1/p')

[ -n "$published" ] ||
  fail "no published artifact could be derived from $workflow; its publish calls moved, so point this gate's extraction at their new shape"

published_ids="$(printf '%s' "$published" | cut -f1)"
platform_ids="$(printf '%s' "$platforms" | cut -f1)"

while IFS=$'\t' read -r id manifest; do
  [ -n "$id" ] || continue
  if ! printf '%s\n' "$declared_ids" | grep -Fxq -- "$id"; then
    fail "$workflow publishes '$id' (from $manifest) and $declarations declares no target for it, so a consumer waiting on it gets no hold; add a [[target]] with id = \"$id\" and manifest = \"$manifest\""
    continue
  fi
  want="$(printf '%s' "$declared" | awk -F'\t' -v id="$id" '$1 == id { print $3 }')"
  [ "$want" = "$manifest" ] ||
    fail "$declarations gives '$id' manifest \"$want\" but $workflow publishes it from $manifest; set that target's manifest = \"$manifest\""
done < <(printf '%s' "$published")

while read -r id; do
  [ -n "$id" ] || continue
  printf '%s\n' "$published_ids" | grep -Fxq -- "$id" && continue
  if printf '%s\n' "$platform_ids" | grep -Fxq -- "$id"; then
    fail "$declarations declares per-platform package '$id' as a target; nothing depends on it by name, so move it into the covers list of the npm target whose optionalDependencies pin it"
  else
    fail "$declarations declares '$id', which $workflow does not publish; remove that [[target]], or restore whatever published it"
  fi
done < <(printf '%s\n' "$declared_ids")

if [ "$(printf '%s\n' "$declared_ids" | grep -Fxf <(printf '%s\n' "$published_ids") || true)" != \
  "$(printf '%s\n' "$published_ids" | grep -Fxf <(printf '%s\n' "$declared_ids") || true)" ]; then
  fail "$declarations lists its targets in a different order than $workflow publishes them ($(printf '%s' "$published_ids" | tr '\n' ' ')); reorder the [[target]] entries to match"
fi

while IFS=$'\t' read -r id manifest; do
  [ -n "$id" ] || continue
  owner="$(printf '%s' "$covered" | awk -F'\t' -v id="$id" '$1 == id { print $2; exit }')"
  if [ -z "$owner" ]; then
    fail "$workflow publishes per-platform package '$id' (from $manifest) and no target covers it; add \"$id\" to the covers list of npm:@skill-test/sdk, whose optionalDependencies resolve it"
    continue
  fi
  owner_manifest="$(printf '%s' "$declared" | awk -F'\t' -v id="$owner" '$1 == id { print $3 }')"
  case "$owner_manifest" in
    *.json) pinned="$(jq -r --arg n "${id#npm:}" '.optionalDependencies[$n] | if type == "string" then . else "" end' "$owner_manifest" 2>/dev/null || true)" ;;
    *) pinned="" ;;
  esac
  [ -n "$pinned" ] ||
    fail "$declarations has $owner cover '$id', but ${owner_manifest:-its manifest} does not pin it in optionalDependencies, so an install never resolves it; add \"${id#npm:}\": \"workspace:*\" there, or move the covers entry to the target that does pin it"
done < <(printf '%s' "$platforms")

while IFS=$'\t' read -r id owner; do
  [ -n "$id" ] || continue
  printf '%s\n' "$platform_ids" | grep -Fxq -- "$id" ||
    fail "$declarations has $owner cover '$id', which $workflow does not publish; drop it from that covers list, or restore whatever published it"
done < <(printf '%s' "$covered")

while IFS=$'\t' read -r id _ manifest; do
  case "$id:$manifest" in
    npm:*.json) ;;
    *) continue ;;
  esac
  [ -f "$manifest" ] || {
    fail "$declarations names manifest $manifest for $id, which does not exist; point it at the package.json that publishes $id"
    continue
  }
  if ! deps="$(jq -r '.optionalDependencies // {} | to_entries[]
      | if (.value | type) == "string" then .key else error("\(.key) is pinned by a non-string spec") end' "$manifest" 2>&1)"; then
    fail "$manifest's optionalDependencies cannot be read ($deps); make it a JSON object of package name to version-spec string"
    continue
  fi
  while read -r dep; do
    [ -n "$dep" ] || continue
    printf '%s\n' "$platform_ids" | grep -Fxq -- "npm:$dep" ||
      fail "$manifest pins '$dep' in optionalDependencies but $workflow never publishes it, so installs of $id cannot resolve it; publish it from the npm job's platform loop or drop the pin"
  done <<<"$deps"
done < <(printf '%s' "$declared")

if [ "$fails" -ne 0 ]; then
  printf 'check-release-targets: %d drift(s) between %s and %s\n' "$fails" "$declarations" "$workflow" >&2
  exit 1
fi
echo "check-release-targets: every artifact publish.yml publishes is declared or covered, and nothing else is"
