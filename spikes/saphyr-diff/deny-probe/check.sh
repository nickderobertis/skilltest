#!/usr/bin/env bash
# cargo deny (the repo's deny.toml) over the post-move dependency set, then
# confirm none of the libyaml-lineage crates is anywhere in it.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../../.." && pwd)"
cargo deny --manifest-path "$here/Cargo.toml" check --config "$root/deny.toml" bans licenses sources advisories
for crate in serde_yaml serde_yaml_ng serde_norway serde_yml unsafe-libyaml unsafe-libyaml-norway libyml; do
  if cargo tree --manifest-path "$here/Cargo.toml" -e normal,build -i "$crate" >/dev/null 2>&1; then
    echo "PRESENT: $crate"
  else
    echo "absent: $crate"
  fi
done
echo "-- serde-saphyr subtree"
cargo tree --manifest-path "$here/Cargo.toml" -e normal,build -p serde-saphyr
echo "-- saphyr subtree"
cargo tree --manifest-path "$here/Cargo.toml" -e normal,build -p saphyr
echo "-- duplicate versions"
cargo tree --manifest-path "$here/Cargo.toml" -e normal,build --duplicates || true
