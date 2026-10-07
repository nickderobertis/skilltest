#!/usr/bin/env bash
# llmlint: ignore-file[new_code_lands_in_a_project] repo-level release gate over release-targets.toml and .github/workflows/publish.yml, neither of which belongs to an Nx package; run workspace-wide from `just check` like scripts/gen-contract.sh --check (AGENTS.md: scripts/*.sh are orchestrator-independent glue).
# Drift gate for release-targets.toml: what it declares against what this
# repository really publishes.
#
# A consumer sequencing work across repositories reads release-targets.toml to
# learn which skilltest artifact to wait on, and a target nobody declared grants
# that consumer no hold at all — silently. So the published set is DERIVED from
# publish.yml's publish calls and the manifests they read, never transcribed.
#
# A per-platform package is a `covers` entry of the target whose release ships
# it, never a target of its own, and it must also be pinned in that target's
# optionalDependencies — the covers list is what a consumer reading the
# declaration sees, the pin is what makes an install resolve the binary.
#
# Fails in both directions — a published name neither declared nor covered, and
# a declared or covered name nothing publishes — naming each drift and its fix.
#
# It also holds every per-platform list to release-platforms.toml, the one
# statement of which platforms a release ships a binary for: the build matrices
# of publish.yml, release.yml and windows-build.yml, publish.yml's Windows
# install proof, the wheel scripts' target
# list and tag map, the npm loop and stager, the platform packages, the covers,
# the SDK's optionalDependencies, .releaserc.json's assets and set-version.sh —
# and bundle-smoke.yml may smoke only declared platforms, on their runners.
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
platforms_file="release-platforms.toml"
release_workflow=".github/workflows/release.yml"
windows_workflow=".github/workflows/windows-build.yml"
smoke_workflow=".github/workflows/bundle-smoke.yml"
dist_script="scripts/build-python-dist.sh"
wheel_script="scripts/build-python-wheel.sh"
set_version="scripts/set-version.sh"
releaserc=".releaserc.json"
sdk_manifest="sdks/typescript/package.json"
DECLARATION_SCHEMA_VERSION=3

fails=0
fail() {
  printf 'release-target drift: %s\n' "$1" >&2
  fails=$((fails + 1))
}

# A TOML table's string `name`, parsed by Python's own TOML reader; empty when
# the file is unreadable, is not valid TOML, or lacks that string — every caller
# fails on empty, naming the manifest. $1 = file, $2 = table.
toml_section_name() {
  local name
  if name="$(python3 -c '
import sys, tomllib
with open(sys.argv[1], "rb") as f:
    name = tomllib.load(f).get(sys.argv[2], {}).get("name")
if isinstance(name, str) and name:
    sys.stdout.write(name)
' "$1" "$2" 2>/dev/null)"; then
    printf '%s' "$name"
  fi
}

# Empty unless the file is exactly one JSON document whose name is a non-empty
# string — output from a parse that later failed is discarded. Every caller
# fails on empty, naming the manifest.
json_name() {
  local name
  if name="$(jq -rs 'if length == 1 and (.[0].name | type) == "string" then .[0].name else "" end' "$1" 2>/dev/null)"; then
    printf '%s' "$name"
  fi
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

for required in "$declarations" "$workflow" "$stager" "$platforms_file" "$release_workflow" \
  "$windows_workflow" "$smoke_workflow" "$dist_script" "$wheel_script" "$set_version" "$releaserc" "$sdk_manifest"; do
  [ -f "$required" ] || {
    echo "check-release-targets: $required is missing; restore it from git — this gate derives the published set from it" >&2
    exit 1
  }
done
command -v jq >/dev/null 2>&1 || {
  echo "check-release-targets: jq is required to read the npm manifests; install jq and retry" >&2
  exit 1
}
python3 -c 'import tomllib' 2>/dev/null || {
  echo "check-release-targets: python3 3.11+ (for tomllib) is required to read the Cargo and pyproject manifests; install it and retry" >&2
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
    *.json)
      pinned="$(jq -rs --arg n "${id#npm:}" 'if length == 1 then .[0].optionalDependencies[$n] | if type == "string" then . else "" end else "" end' "$owner_manifest" 2>/dev/null)" ||
        pinned=""
      ;;
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
  if ! deps="$(jq -rs 'if length != 1 then error("it is not exactly one JSON document") else .[0] end | .optionalDependencies // {} | to_entries[]
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

# One "<target>\t<runner>\t<bin>\t<wheel_tag>\t<npm_package>\t<npm_dir>" per
# platform, or one "!\t<message>" per refusal. Parsed by Python's TOML reader;
# every field must be one non-empty string.
platform_rows="$(python3 -c '
import sys, tomllib
fields = ("target", "runner", "bin", "wheel_tag", "npm_package", "npm_dir")
names = "/".join(fields)
try:
    with open(sys.argv[1], "rb") as f:
        doc = tomllib.load(f)
except (OSError, tomllib.TOMLDecodeError) as e:
    print(f"!\t{sys.argv[1]} is not readable TOML ({e}); restore it from git")
    sys.exit()
rows = doc.get("platform")
if not isinstance(rows, list) or not rows:
    print(f"!\t{sys.argv[1]} declares no [[platform]]; restore its platform list from git")
    sys.exit()
for n, row in enumerate(rows, 1):
    values = [row.get(k) if isinstance(row, dict) else None for k in fields]
    if not all(isinstance(v, str) and v and not set(v) & {"\t", "\n", "\r"} for v in values):
        print(f"!\t{sys.argv[1]} [[platform]] {n} lacks one of {names} as a non-empty string; give it every field")
        continue
    print("\t".join(values))
' "$platforms_file")" || platform_rows="!	could not run python3 over $platforms_file; check that python3 3.11+ is on PATH"

while IFS=$'\t' read -r first rest; do
  [ "$first" = "!" ] && fail "$rest"
done <<<"$platform_rows"
platform_rows="$(printf '%s\n' "$platform_rows" | grep -v '^!' || true)"
# $1 = column (1-based). That column of every declared platform, one per line.
declared_col() { printf '%s\n' "$platform_rows" | sed '/^$/d' | cut -f"$1"; }
# Set operations over newline lists, in one awk each so a failure is the
# command's own exit status rather than a process substitution's lost one.
# $1 = A, $2 = B: the lines of A not in B, each once, in A's order.
set_minus() {
  printf '%s\n\001\n%s\n' "$2" "$1" | awk '
    $0 == "\001" { second = 1; next }
    $0 == "" { next }
    !second { seen[$0] = 1; next }
    !($0 in seen) && !out[$0]++ { print }
  '
}
# $1 = a list: each line it holds more than once, once.
repeated() { printf '%s\n' "$1" | awk '$0 != "" && count[$0]++ == 1 { print }'; }

# $1 = what to call the set in a failure, rest = the set operation to run.
set_op() {
  local what="$1" out
  shift
  out="$("$@")" || {
    echo "check-release-targets: could not compare $what (awk failed); check that awk is on PATH" >&2
    exit 1
  }
  printf '%s' "$out"
}

for col in 1 5 6; do
  dupes="$(set_op "$platforms_file's column $col" repeated "$(declared_col "$col")")"
  while read -r dup; do
    if [ -n "$dup" ]; then fail "$platforms_file declares '$dup' on more than one [[platform]]; each platform is one target, one npm package and one directory, so drop the extra"; fi
  done <<<"$dupes"
done

# $1 = what restates the set, $2 = the declared lines, $3 = its lines, $4 = the
# fix. Fails once per line on either side alone.
compare_platforms() {
  local what="$1" want="$2" got="$3" fix="$4" line missing extra dupes
  missing="$(set_op "$what" set_minus "$want" "$got")"
  extra="$(set_op "$what" set_minus "$got" "$want")"
  dupes="$(set_op "$what" repeated "$got")"
  while read -r line; do
    if [ -n "$line" ]; then fail "$what lacks '$line', which $platforms_file declares; $fix"; fi
  done <<<"$missing"
  while read -r line; do
    if [ -n "$line" ]; then fail "$what has '$line', which $platforms_file does not declare; $fix"; fi
  done <<<"$extra"
  while read -r line; do
    if [ -n "$line" ]; then fail "$what lists '$line' more than once; drop the extra"; fi
  done <<<"$dupes"
}

# A workflow matrix's one-line `- { target: T, os: R[, bin: B] }` rows as
# "T\tR[\tB]". $1 = workflow, $2 = job id (empty: the whole file).
matrix_rows() {
  local body
  if [ -n "$2" ]; then
    body="$(awk -v job="  $2:" '
      $0 == job { inside = 1; next }
      inside && /^  [A-Za-z0-9_-]+:$/ { exit }
      inside { print }
    ' "$1")" || {
      echo "check-release-targets: could not read $1; check its permissions" >&2
      exit 1
    }
  else
    body="$(cat "$1")" || {
      echo "check-release-targets: could not read $1; check its permissions" >&2
      exit 1
    }
  fi
  printf '%s\n' "$body" | sed -n 's/^ *- { *target: *\([^ ,}]*\), *os: *\([^ ,}]*\)\(, *bin: *\([^ ,}]*\)\)\{0,1\} *}$/\1	\2	\4/p' | sed 's/	$//'
}

declared_builds="$(printf '%s\n' "$platform_rows" | sed '/^$/d' | cut -f1-3)"
matrix_fix="write one '- { target: <triple>, os: <runner>, bin: <file> }' row per $platforms_file platform, with its runner and bin"
for spec in "$workflow binaries" "$release_workflow upload"; do
  file="${spec% *}"
  job="${spec#* }"
  rows="$(matrix_rows "$file" "$job")"
  [ -n "$rows" ] || fail "$file's $job job has no one-line '- { target: ..., os: ..., bin: ... }' matrix rows; restore that shape so this gate can read its platforms"
  compare_platforms "$file's $job matrix" "$declared_builds" "$rows" "$matrix_fix"
done

windows_builds="$(printf '%s\n' "$declared_builds" | awk -F'\t' '$1 ~ /-windows-/')"
for spec in "$windows_workflow:" "$workflow:verify-windows"; do
  file="${spec%:*}"
  job="${spec#*:}"
  what="$file's ${job:+$job }matrix"
  rows="$(matrix_rows "$file" "$job")"
  [ -n "$rows" ] || fail "$what has no one-line '- { target: ..., os: ..., bin: ... }' rows; restore that shape so this gate can read its platforms"
  compare_platforms "$what" "$windows_builds" "$rows" "it proves every declared Windows platform, and only those; $matrix_fix"
done

rows="$(matrix_rows "$smoke_workflow" "")"
[ -n "$rows" ] || fail "$smoke_workflow has no one-line '- { target: ..., os: ... }' matrix rows; restore that shape so this gate can read its platforms"
while IFS=$'\t' read -r target runner _; do
  [ -n "$target" ] || continue
  want="$(printf '%s\n' "$declared_builds" | awk -F'\t' -v t="$target" '$1 == t { print $2 }')"
  if [ -z "$want" ]; then
    fail "$smoke_workflow smokes '$target', which $platforms_file does not declare; smoke only a platform the release ships, or declare it"
  elif [ "$want" != "$runner" ]; then
    fail "$smoke_workflow smokes '$target' on '$runner' but $platforms_file builds it on '$want'; smoke it on its declared runner"
  fi
done <<<"$rows"

# $1 = file, $2 = sed script. The file's matching lines, or an exit naming it.
read_lines() {
  sed -n "$2" "$1" || {
    echo "check-release-targets: could not read $1; check its permissions or restore it from git" >&2
    exit 1
  }
}

dist_targets="$(read_lines "$dist_script" 's/^targets="\([^"]*\)"$/\1/p' | tr ' ' '\n')"
[ -n "$dist_targets" ] || fail "$dist_script has no 'targets=\"...\"' line; restore it so this gate can read which platform wheels it builds"
compare_platforms "$dist_script's targets" "$(declared_col 1)" "$dist_targets" "list every declared target in its targets=\"...\" line"

wheel_tags="$(read_lines "$wheel_script" 's/^\([A-Za-z0-9_.-]*\)) plat="\([^"]*\)" ;;$/\1	\2/p')"
compare_platforms "$wheel_script's tag map" "$(printf '%s\n' "$platform_rows" | sed '/^$/d' | cut -f1,4)" "$wheel_tags" \
  "give each declared target a '<target>) plat=\"<wheel_tag>\" ;;' arm with its declared wheel_tag"

stager_dirs="$(read_lines "$stager" 's/^\([A-Za-z0-9_.-]*\)) pkg="\([^"]*\)" ;;$/\1	\2/p')"
compare_platforms "$stager's package map" "$(printf '%s\n' "$platform_rows" | sed '/^$/d' | cut -f1,6)" "$stager_dirs" \
  "give each declared target a '<target>) pkg=\"<npm_dir>\" ;;' arm with its declared npm_dir"

compare_platforms "$workflow's npm 'for target in' loop" "$(declared_col 1)" "$targets" "loop over every declared target"

committed_packages=""
for manifest in sdks/typescript/platforms/*/package.json; do
  [ -f "$manifest" ] || continue
  dir="${manifest#sdks/typescript/platforms/}"
  committed_packages="${committed_packages}${dir%/package.json}	$(json_name "$manifest")
"
done
compare_platforms "sdks/typescript/platforms/" "$(printf '%s\n' "$platform_rows" | awk -F'\t' 'NF { print $6 "\t" $5 }')" "$committed_packages" \
  "commit one <npm_dir>/package.json per declared platform, named its npm_package, and none other"

sdk_covers="$(printf '%s' "$covered" | awk -F'\t' '$2 == "npm:@skill-test/sdk" { sub(/^npm:/, "", $1); print $1 }')"
compare_platforms "$declarations's npm:@skill-test/sdk covers" "$(declared_col 5)" "$sdk_covers" "cover \"npm:<npm_package>\" for every declared platform"

if optional="$(jq -rs 'if length != 1 then error("it is not exactly one JSON document") else .[0] end
    | .optionalDependencies // {} | if type == "object" then keys[] else error("optionalDependencies is not an object") end' "$sdk_manifest" 2>&1)"; then
  compare_platforms "$sdk_manifest's optionalDependencies" "$(declared_col 5)" "$optional" "pin \"<npm_package>\": \"workspace:*\" for every declared platform"
else
  fail "$sdk_manifest's optionalDependencies cannot be read ($optional); make it one JSON object whose optionalDependencies maps each platform package to a version spec"
fi

if assets="$(jq -rs 'if length != 1 then error("it is not exactly one JSON document") else .[0] end
    | [.plugins[]? | select(type == "array" and .[0] == "@semantic-release/git")]
    | if length == 1 then .[0][1].assets[] else error("it configures @semantic-release/git \(length) times, not once") end' "$releaserc" 2>&1)"; then
  assets="$(printf '%s\n' "$assets" | sed -n 's|^sdks/typescript/platforms/\([^/]*\)/package.json$|\1|p')"
  compare_platforms "$releaserc's @semantic-release/git assets" "$(declared_col 6)" "$assets" \
    "list sdks/typescript/platforms/<npm_dir>/package.json for every declared platform, so the release commit carries its version"
else
  fail "$releaserc's @semantic-release/git assets cannot be read ($assets); restore that plugin entry with an assets list"
fi

grep -Fxq 'for pkg in sdks/typescript/platforms/*/package.json; do' "$set_version" ||
  fail "$set_version no longer versions every platform package through its 'for pkg in sdks/typescript/platforms/*/package.json; do' loop, so a declared platform's package can be left on the previous version; restore that loop"

if [ "$fails" -ne 0 ]; then
  printf 'check-release-targets: %d drift(s) between %s, %s and what they declare\n' "$fails" "$declarations" "$platforms_file" >&2
  exit 1
fi
echo "check-release-targets: every artifact publish.yml publishes is declared or covered, and every platform list matches $platforms_file"
