#!/usr/bin/env bash
# Run the serde_yaml vs serde-saphyr differential harness (release build, so the
# parse timings are meaningful). Prints every difference per document kind.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec cargo run --quiet --release --manifest-path "$here/Cargo.toml" -- "$@"
