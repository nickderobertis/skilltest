#!/usr/bin/env bash
# The resolver invokes the real upstream matrix reader, pinned with its workflow.
set -euo pipefail
cd "$(dirname "$0")/.." || { echo 'screencomp setup: cannot enter repository; restore a complete checkout' >&2; exit 1; }
if command -v screencomp >/dev/null 2>&1 && [ "$(screencomp --version)" = 'screencomp 0.4.2' ]; then exit 0; fi
if [ -x .github/.tools/screencomp ]; then
  [ "$(.github/.tools/screencomp --version)" = 'screencomp 0.4.2' ] && exit 0
fi
case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) target=x86_64-unknown-linux-gnu ;;
  Linux-aarch64) target=aarch64-unknown-linux-gnu ;;
  Darwin-x86_64) target=x86_64-apple-darwin ;;
  Darwin-arm64) target=aarch64-apple-darwin ;;
  *) echo 'screencomp setup: unsupported host; install screencomp 0.4.2 and rerun github-workflows:test' >&2; exit 1 ;;
esac
mkdir -p .github/.tools || { echo 'screencomp setup: cannot create .github/.tools; check checkout permissions' >&2; exit 1; }
asset="screencomp-v0.4.2-$target.tar.gz"
url="https://github.com/nickderobertis/screencomp/releases/download/v0.4.2/$asset"
expected="$(node -e 'const p=require("./.github/screencomp-lock.json");process.stdout.write(p[process.argv[1]])' "$target")" || { echo 'screencomp setup: cannot read checksum lock; restore .github/screencomp-lock.json' >&2; exit 1; }
[[ "$expected" =~ ^[a-f0-9]{64}$ ]] || { echo 'screencomp setup: missing checksum; restore the declared target in screencomp-lock.json' >&2; exit 1; }
curl -fsSL "$url" -o ".github/.tools/$asset" || { echo 'screencomp setup: download failed; retry GitHub transport or install screencomp 0.4.2' >&2; exit 1; }
actual="$(python3 -c 'import hashlib,sys;print(hashlib.sha256(open(sys.argv[1],"rb").read()).hexdigest())' ".github/.tools/$asset")" || { echo 'screencomp setup: cannot hash download; check its readability and retry' >&2; exit 1; }
[ "$actual" = "$expected" ] || { echo 'screencomp setup: upstream archive checksum changed; inspect the pinned release before updating the lock' >&2; exit 1; }
tar -xzf ".github/.tools/$asset" -C .github/.tools || { echo 'screencomp setup: cannot extract archive; inspect the download and retry' >&2; exit 1; }
rm ".github/.tools/$asset" || { echo 'screencomp setup: cannot remove download; check .github/.tools permissions' >&2; exit 1; }
[ "$(.github/.tools/screencomp --version)" = 'screencomp 0.4.2' ] || { echo 'screencomp setup: wrong binary version; restore the pinned archive' >&2; exit 1; }
