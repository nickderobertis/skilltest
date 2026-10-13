#!/usr/bin/env bash
# Refuse a tag/version/head mismatch before using a release build's artifacts.
set -euo pipefail
if [ "$#" -ne 4 ] || ! [[ "$2" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$ ]] ||
  ! [[ "$3" =~ ^[a-f0-9]{40}$ ]] || ! [[ "$4" =~ ^[a-f0-9]{40}$ ]] ||
  [ "$1" != "v$2" ] || [ "$3" != "$4" ]; then
  echo 'release identity: tag, manifest version and exact commit disagree; rebuild from the matching immutable tag' >&2
  exit 1
fi
