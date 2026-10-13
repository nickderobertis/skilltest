#!/usr/bin/env bash
# Native packaging journey through the production builder and fresh consumers.
set -euo pipefail
cd "$(dirname "$0")/.." || { echo 'release-product-test: cannot enter repository; restore a complete checkout' >&2; exit 1; }
target="$(rustc -vV | sed -n 's/^host: //p')"
work="$(mktemp -d)" || { echo 'release-product-test: cannot create scratch directory; check TMPDIR permissions and free space' >&2; exit 1; }
exec 3>&1 4>&2
finish() {
  journey_status=$?
  if [ "$journey_status" -ne 0 ]; then
    cat "$work/journey.log" >&4 || echo 'release-product-test: cannot read journey log; check scratch permissions' >&4
    echo 'release-product-test: journey failed; rerun just release-product-test and inspect the named artifact or consumer error' >&4
  fi
  rm -rf "$work" || { echo 'release-product-test: cannot remove scratch directory; check its permissions' >&4; exit 1; }
  exit "$journey_status"
}
trap finish EXIT
exec >"$work/journey.log" 2>&1
cargo build --release --locked -p skilltest-cli --bin skilltest --target "$target"
cargo build --release --locked -p skilltest-cli --bin skilltest-fake-provider --features fake-provider --target "$target"
binary="target/$target/release/skilltest"
provider="target/$target/release/skilltest-fake-provider"
bash scripts/stage-release-product.sh "$target" "$binary" "$work/products"
version="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -1)"
bash scripts/verify-release-archive.sh "$work/products" "$target" skilltest "$version"
bash scripts/smoke-python-bundle.sh "$target" "$binary" "$provider" "$work/products"
bash scripts/smoke-npm-bundle.sh "$target" "$binary" "$provider" "$work/products"
cp -r "$work/products" "$work/native-products"
# Tamper with a payload after packing: the same production verifier must refuse it.
python3 - "$work/products" <<'PY'
from pathlib import Path
import sys
import zipfile
wheel, = Path(sys.argv[1]).glob('*.whl')
with zipfile.ZipFile(wheel) as z:
    entries = {name: z.read(name) for name in z.namelist()}
entries['skilltest_sdk/_bin/skilltest'] = b'tampered binary'
with zipfile.ZipFile(wheel, 'w') as z:
    for name, data in entries.items():
        z.writestr(name, data)
PY
pkg="$(bash scripts/stage-npm-binary.sh "$target" "$binary")"
if python3 scripts/stage-and-verify-release-product.py "$target" "$binary" "$work/products" "$pkg" >"$work/failure.log" 2>&1; then
  echo 'release-product-test: tampered wheel was accepted; restore binary identity verification' >&2
  exit 1
fi
grep -F 'wheel binary differs' "$work/failure.log" >/dev/null || { cat "$work/failure.log" >&2; echo 'release-product-test: expected wheel binary differs; inspect production verifier diagnostics before adjusting the fixture' >&2; exit 1; }
echo 'release-product-test: real native archive/wheel/npm install and failure journeys passed'
# Foreign platforms: exercise the real packager's declared architecture and payload
# contracts, using header fixtures. These bytes are never claimed to execute.
python3 - "$work/contracts" <<'PY'
from pathlib import Path
import struct
import subprocess
import sys
import tomllib
import tarfile
import io
import json
import zipfile
root = Path(sys.argv[1])
for platform in tomllib.load(open('release-platforms.toml', 'rb'))['platform']:
    target = platform['target']
    directory = root / target
    directory.mkdir(parents=True)
    data = bytearray(128)
    if 'apple' in target:
        data[:8] = struct.pack('<II', 0xfeedfacf, 0x01000007 if target.startswith('x86_64') else 0x0100000c)
    elif 'linux' in target:
        data[:4] = b'\x7fELF'
        data[18:20] = struct.pack('<H', 62 if target.startswith('x86_64') else 183)
    else:
        data[:2] = b'MZ'
        data[60:64] = struct.pack('<I', 64)
        data[64:70] = b'PE\0\0' + struct.pack('<H', 0x8664 if target.startswith('x86_64') else 0xaa64)
    binary = directory / platform['bin']
    binary.write_bytes(data)
    subprocess.run(['bash', 'scripts/stage-release-product.sh', target, str(binary), str(directory / 'products')], check=True)
    package = 'sdks/typescript/platforms/' + platform['npm_dir']
    def reject(needle, source_dir=package):
        result = subprocess.run(['python3', 'scripts/stage-and-verify-release-product.py', target, str(binary), str(directory / 'products'), str(source_dir)], capture_output=True, text=True)
        if result.returncode == 0 or needle not in result.stderr:
            raise RuntimeError(f'expected rejection {needle}: {result.stdout} {result.stderr}')
    wheel, = (directory / 'products').glob('*.whl')
    wheel_bytes = wheel.read_bytes()
    with zipfile.ZipFile(wheel) as z:
        wheel_entries = {name: z.read(name) for name in z.namelist()}
    def rewrite_wheel(kind):
        with zipfile.ZipFile(wheel, 'w') as z:
            for name, content in wheel_entries.items():
                if kind == 'metadata' and name.endswith('.dist-info/METADATA'):
                    content = content.replace(b'Version: ', b'Incorrect-Version: ')
                z.writestr(name, content)
            if kind == 'provider':
                z.writestr('skilltest_sdk/_bin/skilltest-fake-provider', b'not a shipped product')
    rewrite_wheel('provider')
    reject('wheel contains fake provider')
    rewrite_wheel('metadata')
    reject('wrong wheel version')
    wheel.write_bytes(wheel_bytes)
    original = binary.read_bytes()
    binary.write_bytes(bytes(len(original)))
    reject('architecture')
    binary.write_bytes(original)
    carrier, = (directory / 'products').glob('*.tgz')
    packed_bytes = carrier.read_bytes()
    with tarfile.open(carrier) as tar:
        entries = [(member, tar.extractfile(member).read()) for member in tar.getmembers()]
    def rewrite(corrupt):
        with tarfile.open(carrier, 'w:gz') as tar:
            for member, content in entries:
                if member.name.endswith('/' + platform['bin']) and corrupt == 'binary':
                    content = b'tampered npm payload'
                if member.name == 'package/package.json' and corrupt == 'identity':
                    manifest = json.loads(content)
                    manifest['version'] = '99.99.99'
                    content = json.dumps(manifest).encode()
                if member.name == 'package/package.json' and corrupt in ('cpu', 'os'):
                    manifest = json.loads(content)
                    manifest[corrupt] = ['arm64' if target.startswith('x86_64') else 'x64'] if corrupt == 'cpu' else ['linux' if 'linux' not in target else 'win32']
                    content = json.dumps(manifest).encode()
                member.size = len(content)
                tar.addfile(member, io.BytesIO(content))
            if corrupt == 'extra':
                member = tarfile.TarInfo('package/unexpected')
                member.size = 1
                tar.addfile(member, io.BytesIO(b'x'))
    rewrite('binary')
    reject('npm binary differs')
    rewrite('identity')
    reject('packed npm identity differs from source')
    source_dir = directory / 'metadata-source' / platform['npm_dir']
    source_dir.mkdir(parents=True)
    source = json.loads((Path(package) / 'package.json').read_text())
    for field, message in [('cpu', 'wrong npm architecture'), ('os', 'wrong npm OS')]:
        changed = dict(source)
        changed[field] = ['arm64' if target.startswith('x86_64') else 'x64'] if field == 'cpu' else ['linux' if 'linux' not in target else 'win32']
        (source_dir / 'package.json').write_text(json.dumps(changed))
        rewrite(field)
        reject(message, source_dir)
    rewrite('extra')
    reject('unexpected npm contents')
    carrier.write_bytes(packed_bytes)
PY
node .github/tests/release-journey.mjs "$work" "$target"
echo 'release-product-test: native install/run and declared platform packaging/failure contracts passed; foreign headers not executed' >&3
