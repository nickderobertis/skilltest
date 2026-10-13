#!/usr/bin/env bash
# The resolver invokes the real upstream matrix reader, pinned with its workflow.
set -euo pipefail
cd "$(dirname "$0")/.."
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
expected="$(node -e 'const p=require("./.github/screencomp-lock.json");process.stdout.write(p[process.argv[1]])' "$target")"
curl -fsSL "$url" -o ".github/.tools/$asset" || { echo 'screencomp setup: download failed; retry GitHub transport or install screencomp 0.4.2' >&2; exit 1; }
actual="$(python3 -c 'import hashlib,sys;print(hashlib.sha256(open(sys.argv[1],"rb").read()).hexdigest())' ".github/.tools/$asset")"
[ "$actual" = "$expected" ] || { echo 'screencomp setup: upstream archive checksum changed; inspect the pinned release before updating the lock' >&2; exit 1; }
tar -xzf ".github/.tools/$asset" -C .github/.tools || { echo 'screencomp setup: cannot extract archive; inspect the download and retry' >&2; exit 1; }
rm ".github/.tools/$asset"
[ "$(.github/.tools/screencomp --version)" = 'screencomp 0.4.2' ] || { echo 'screencomp setup: wrong binary version; restore the pinned archive' >&2; exit 1; }
