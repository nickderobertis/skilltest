"""Stage an archive and verify all package identities and binary payloads."""

from dataclasses import asdict, dataclass
import hashlib
import json
from pathlib import Path
import re
import struct
import sys
import tarfile
import tomllib
import zipfile

VERSION = re.compile(r"^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$")


def require(condition: bool, reason: str) -> None:
    if not condition:
        raise ValueError(reason)


@dataclass(frozen=True)
class Platform:
    target: str
    runner: str
    bin: str
    wheel_tag: str
    npm_package: str
    npm_dir: str

    @classmethod
    def parse(cls, raw: object) -> "Platform":
        require(isinstance(raw, dict), "platform must be a table")
        if not isinstance(raw, dict):
            raise ValueError("platform must be a table")
        names = ("target", "runner", "bin", "wheel_tag", "npm_package", "npm_dir")
        values = [raw.get(key) for key in names]
        require(
            all(isinstance(v, str) and v for v in values), "missing platform fields"
        )
        require(set(raw) == set(names), "unexpected platform fields")
        return cls(**dict(zip(names, values, strict=True)))


@dataclass(frozen=True)
class NpmIdentity:
    name: str
    version: str
    cpu: tuple[str, ...]
    os: tuple[str, ...]

    @classmethod
    def parse(cls, raw: object) -> "NpmIdentity":
        if not isinstance(raw, dict):
            raise ValueError("npm manifest must be an object")
        name, version, cpu, os = (raw.get(k) for k in ("name", "version", "cpu", "os"))
        require(
            isinstance(name, str) and name.startswith("@skill-test/cli-"),
            "invalid npm name",
        )
        require(
            isinstance(version, str) and bool(VERSION.fullmatch(version)),
            "invalid npm version",
        )
        require(
            isinstance(cpu, list) and cpu in (["x64"], ["arm64"]), "invalid npm CPU"
        )
        require(
            isinstance(os, list) and os in (["linux"], ["darwin"], ["win32"]),
            "invalid npm OS",
        )
        return cls(name, version, tuple(cpu), tuple(os))


@dataclass(frozen=True)
class PayloadEvidence:
    target: str
    version: str
    binary_sha256: str
    payload_sha256: dict[str, str]
    archive: str
    wheel: str
    npm: str


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def main() -> None:
    require(
        len(sys.argv) == 5, "usage: <target> <binary> <output-dir> <npm-package-dir>"
    )
    target, binary_arg, output, package = sys.argv[1:]
    records = tomllib.loads(Path("release-platforms.toml").read_text()).get("platform")
    require(isinstance(records, list), "release platforms must be an array")
    platforms = [Platform.parse(raw) for raw in records]
    require(
        len({p.target for p in platforms}) == len(platforms),
        "duplicate platform target",
    )
    platform = next(p for p in platforms if p.target == target)
    require(
        platform.bin == ("skilltest.exe" if "windows" in target else "skilltest"),
        "wrong binary filename",
    )
    require(Path(package).name == platform.npm_dir, "wrong staged npm directory")
    binary = Path(binary_arg)
    require(
        binary.is_file() and not binary.is_symlink(), "binary must be a regular file"
    )
    data = binary.read_bytes()
    if "apple" in target:
        cpu = struct.unpack("<I", data[4:8])[0]
        require(
            data[:4] == b"\xcf\xfa\xed\xfe"
            and cpu == (0x01000007 if target.startswith("x86_64") else 0x0100000C),
            "wrong Mach-O architecture",
        )
    elif "linux" in target:
        require(
            data[:4] == b"\x7fELF"
            and struct.unpack("<H", data[18:20])[0]
            == (62 if target.startswith("x86_64") else 183),
            "wrong ELF architecture",
        )
    else:
        offset = struct.unpack("<I", data[60:64])[0]
        require(
            data[:2] == b"MZ"
            and data[offset : offset + 4] == b"PE\0\0"
            and struct.unpack("<H", data[offset + 4 : offset + 6])[0]
            == (0x8664 if target.startswith("x86_64") else 0xAA64),
            "wrong PE architecture",
        )
    out = Path(output)
    version = tomllib.loads(Path("Cargo.toml").read_text())["workspace"]["package"][
        "version"
    ]
    require(
        isinstance(version, str) and bool(VERSION.fullmatch(version)),
        "invalid manifest version",
    )
    (wheel,) = out.glob(f"skilltest_sdk-{version}-py3-none-{platform.wheel_tag}.whl")
    with zipfile.ZipFile(wheel) as z:
        wheel_data = z.read(f"skilltest_sdk/_bin/{platform.bin}")
        require(wheel_data == data, "wheel binary differs")
        require(
            not any("_bin/skilltest-fake-provider" in name for name in z.namelist()),
            "wheel contains fake provider",
        )
        (metadata,) = [n for n in z.namelist() if n.endswith(".dist-info/METADATA")]
        require(
            f"Version: {version}\n" in z.read(metadata).decode(), "wrong wheel version"
        )
    (packed,) = out.glob("skill-test-cli-*.tgz")
    with tarfile.open(packed) as t:
        payload = t.extractfile(f"package/bin/{platform.bin}")
        require(payload is not None, "npm binary missing")
        npm_data = payload.read()
        require(npm_data == data, "npm binary differs")
        manifest_stream = t.extractfile("package/package.json")
        require(manifest_stream is not None, "npm manifest missing")
        manifest = NpmIdentity.parse(json.load(manifest_stream))
        source = NpmIdentity.parse(
            json.loads((Path(package) / "package.json").read_text())
        )
        require(manifest == source, "packed npm identity differs from source")
        require(
            manifest.name == platform.npm_package and manifest.version == version,
            "wrong npm identity/version",
        )
        require(
            manifest.cpu == ("x64" if target.startswith("x86_64") else "arm64",),
            "wrong npm architecture",
        )
        require(
            manifest.os
            == (
                "darwin"
                if "apple" in target
                else "win32"
                if "windows" in target
                else "linux",
            ),
            "wrong npm OS",
        )
        require(
            set(t.getnames())
            == {"package/package.json", f"package/bin/{platform.bin}"},
            "unexpected npm contents",
        )
    asset = f"skilltest-{target}" + (".zip" if "windows" in target else ".tar.gz")
    archive = out / asset
    if asset.endswith(".zip"):
        with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as z:
            z.write(binary, platform.bin)
        with zipfile.ZipFile(archive) as z:
            archive_data = z.read(platform.bin)
    else:
        with tarfile.open(archive, "w:gz") as t:
            t.add(binary, arcname=platform.bin, recursive=False)
        with tarfile.open(archive) as t:
            archive_data = t.extractfile(platform.bin).read()
    require(archive_data == data, "archive binary differs")
    (out / (asset + ".sha256")).write_text(f"{digest(archive.read_bytes())}  {asset}\n")
    evidence = PayloadEvidence(
        target,
        version,
        digest(data),
        {
            "archive": digest(archive_data),
            "wheel": digest(wheel_data),
            "npm": digest(npm_data),
        },
        asset,
        wheel.name,
        packed.name,
    )
    (out / "payload.json").write_text(json.dumps(asdict(evidence), indent=2) + "\n")
    print(f"release product ({target}): archive/wheel/npm payloads agree")


if __name__ == "__main__":
    try:
        main()
    except (
        ValueError,
        KeyError,
        StopIteration,
        OSError,
        struct.error,
        TypeError,
    ) as error:
        sys.exit(
            f"release product: {error}; rebuild the declared target and restage its products"
        )
