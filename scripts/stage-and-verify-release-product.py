"""Stage an archive and verify all package identities and binary payloads."""

from dataclasses import asdict, dataclass
from email.parser import Parser
import hashlib
import json
from pathlib import Path
import re
import struct
import sys
import tarfile
import tomllib
from typing import NamedTuple, NewType
import zipfile

VERSION = re.compile(r"^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$")

# A declared Rust target triple, a lockstep release version and a hex SHA-256.
Target = NewType("Target", str)
Version = NewType("Version", str)
Sha256 = NewType("Sha256", str)


def require(condition: bool, reason: str) -> None:
    if not condition:
        raise ValueError(reason)


def text(value: object, where: str) -> str:
    if not isinstance(value, str) or not value:
        raise ValueError(f"{where} must be a nonempty string")
    return value


def string_array(value: object, where: str) -> tuple[str, ...]:
    if not isinstance(value, list):
        raise ValueError(f"{where} must be an array")
    return tuple(text(item, where) for item in value)


@dataclass(frozen=True)
class Platform:
    target: Target
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
        return cls(
            Target(text(raw.get("target"), "target")),
            *(text(raw.get(key), key) for key in names[1:]),
        )


@dataclass(frozen=True)
class NpmIdentity:
    name: str
    version: Version
    cpu: tuple[str, ...]
    os: tuple[str, ...]

    @classmethod
    def parse(cls, raw: object) -> "NpmIdentity":
        if not isinstance(raw, dict):
            raise ValueError("npm manifest must be an object")
        name = text(raw.get("name"), "npm name")
        version = text(raw.get("version"), "npm version")
        cpu = string_array(raw.get("cpu"), "npm CPU")
        os = string_array(raw.get("os"), "npm OS")
        require(name.startswith("@skill-test/cli-"), "invalid npm name")
        require(bool(VERSION.fullmatch(version)), "invalid npm version")
        require(cpu in (("x64",), ("arm64",)), "invalid npm CPU")
        require(os in (("linux",), ("darwin",), ("win32",)), "invalid npm OS")
        return cls(name, Version(version), tuple(cpu), tuple(os))


@dataclass(frozen=True)
class PayloadDigests:
    archive: Sha256
    wheel: Sha256
    npm: Sha256


@dataclass(frozen=True)
class PayloadEvidence:
    target: Target
    version: Version
    binary_sha256: Sha256
    payload_sha256: PayloadDigests
    archive: str
    wheel: str
    npm: str


def digest(data: bytes) -> Sha256:
    return Sha256(hashlib.sha256(data).hexdigest())


def release_version() -> Version:
    value = tomllib.loads(Path("Cargo.toml").read_text())["workspace"]["package"][
        "version"
    ]
    version = text(value, "manifest version")
    require(bool(VERSION.fullmatch(version)), "invalid manifest version")
    return Version(version)


def asset_name(platform: Platform) -> str:
    return f"skilltest-{platform.target}" + (
        ".zip" if "windows" in platform.target else ".tar.gz"
    )


class NpmPlatform(NamedTuple):
    cpu: tuple[str, ...]
    os: tuple[str, ...]


def npm_platform(target: Target) -> NpmPlatform:
    """The cpu and os fields an npm carrier for a declared target must state."""
    cpu = ("x64" if target.startswith("x86_64") else "arm64",)
    match target.split("-"):
        case [_, "apple", "darwin"]:
            return NpmPlatform(cpu, ("darwin",))
        case [_, "pc", "windows", "msvc"]:
            return NpmPlatform(cpu, ("win32",))
        case [_, "unknown", "linux", "gnu"]:
            return NpmPlatform(cpu, ("linux",))
        case _:
            raise ValueError("unsupported npm target")


def staged_binary(platform: Platform, directory: Path) -> bytes:
    """The binary a staged archive carries, after its checksum and contents agree."""
    asset = asset_name(platform)
    released = directory / asset
    require(released.is_file(), f"missing staged archive: {released}")
    checksum = (directory / (asset + ".sha256")).read_text()
    require(
        checksum == f"{digest(released.read_bytes())}  {asset}\n",
        "staged archive checksum differs",
    )
    if asset.endswith(".zip"):
        with zipfile.ZipFile(released) as packed:
            require(packed.namelist() == [platform.bin], "unexpected archive contents")
            return packed.read(platform.bin)
    with tarfile.open(released) as packed:
        members = packed.getmembers()
        require(
            len(members) == 1
            and members[0].name == platform.bin
            and members[0].isfile(),
            "unexpected archive contents",
        )
        payload = packed.extractfile(members[0])
        if payload is None:
            raise ValueError("archive binary missing")
        return payload.read()


def check_carriers(platforms: list[Platform], products: Path) -> None:
    """Hold every staged npm carrier to its declared identity and archive binary."""
    version = release_version()
    expected_carriers = set()
    for platform in platforms:
        directory = products / platform.target
        carriers = list(directory.glob("skill-test-cli-*.tgz"))
        require(
            len(carriers) == 1,
            f"missing or ambiguous npm carrier for {platform.target}; restage release products",
        )
        expected_carriers.add(carriers[0])
        binary_data = staged_binary(platform, directory)
        with tarfile.open(carriers[0]) as packed:
            members = packed.getmembers()
            names = [member.name for member in members]
            require(
                len(names) == len(set(names))
                and set(names)
                == {"package/package.json", f"package/bin/{platform.bin}"}
                and all(member.isfile() for member in members),
                "unexpected npm contents",
            )
            manifest_stream = packed.extractfile("package/package.json")
            payload = packed.extractfile(f"package/bin/{platform.bin}")
            if manifest_stream is None or payload is None:
                raise ValueError("npm carrier member missing")
            manifest = NpmIdentity.parse(json.load(manifest_stream))
            require(
                (
                    manifest.name,
                    manifest.version,
                    NpmPlatform(manifest.cpu, manifest.os),
                )
                == (platform.npm_package, version, npm_platform(platform.target)),
                f"packed carrier identity differs from declared target/version: {carriers[0]}",
            )
            require(payload.read() == binary_data, "npm binary differs")
    require(
        set(products.rglob("*.tgz")) == expected_carriers,
        "unexpected staged npm carrier",
    )


def main() -> None:
    check_wheels = len(sys.argv) == 3 and sys.argv[1] == "--check-wheels"
    carriers = len(sys.argv) == 3 and sys.argv[1] == "--check-carriers"
    require(
        check_wheels or carriers or len(sys.argv) == 5,
        "usage: <target> <binary> <output-dir> <npm-package-dir>, "
        "--check-wheels <products-dir> or --check-carriers <products-dir>",
    )
    records = tomllib.loads(Path("release-platforms.toml").read_text()).get("platform")
    if not isinstance(records, list):
        raise ValueError("release platforms must be an array")
    platforms = [Platform.parse(raw) for raw in records]
    require(
        len({p.target for p in platforms}) == len(platforms),
        "duplicate platform target",
    )
    if carriers:
        check_carriers(platforms, Path(sys.argv[2]))
        return
    if check_wheels:
        version = release_version()
        expected_wheels = set()
        for platform in platforms:
            expected = (
                Path(sys.argv[2])
                / platform.target
                / f"skilltest_sdk-{version}-py3-none-{platform.wheel_tag}.whl"
            )
            expected_wheels.add(expected)
            require(expected.is_file(), f"missing staged platform wheel: {expected}")
            binary_data = staged_binary(platform, expected.parent)
            with zipfile.ZipFile(expected) as archive:
                names = archive.namelist()
                require(len(names) == len(set(names)), "duplicate wheel member")
                require(
                    not any("_bin/skilltest-fake-provider" in name for name in names),
                    "wheel contains fake provider",
                )
                require(
                    archive.read(f"skilltest_sdk/_bin/{platform.bin}") == binary_data,
                    "wheel binary differs",
                )
                metadata_names = [
                    name
                    for name in archive.namelist()
                    if name.endswith(".dist-info/METADATA")
                ]
                require(len(metadata_names) == 1, f"invalid wheel metadata: {expected}")
                headers = Parser().parsestr(archive.read(metadata_names[0]).decode())
                require(
                    headers.get_all("Name", []) == ["skilltest-sdk"]
                    and headers.get_all("Version", []) == [version],
                    f"invalid staged wheel identity: {expected}",
                )
        require(
            set(Path(sys.argv[2]).rglob("*.whl")) == expected_wheels,
            "unexpected staged wheel",
        )
        return
    target, binary_arg, output, package = sys.argv[1:]
    platform = next((p for p in platforms if p.target == target), None)
    if platform is None:
        raise ValueError(f"undeclared release target: {target}")
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
    match target.split("-"):
        case [arch, "apple", "darwin"] if arch in ("x86_64", "aarch64"):
            cpu = struct.unpack("<I", data[4:8])[0]
            require(
                data[:4] == b"\xcf\xfa\xed\xfe"
                and cpu == (0x01000007 if target.startswith("x86_64") else 0x0100000C),
                "wrong Mach-O architecture",
            )
        case [arch, "unknown", "linux", "gnu"] if arch in ("x86_64", "aarch64"):
            require(
                data[:4] == b"\x7fELF"
                and struct.unpack("<H", data[18:20])[0]
                == (62 if target.startswith("x86_64") else 183),
                "wrong ELF architecture",
            )
        case [arch, "pc", "windows", "msvc"] if arch in ("x86_64", "aarch64"):
            offset = struct.unpack("<I", data[60:64])[0]
            require(
                data[:2] == b"MZ"
                and data[offset : offset + 4] == b"PE\0\0"
                and struct.unpack("<H", data[offset + 4 : offset + 6])[0]
                == (0x8664 if target.startswith("x86_64") else 0xAA64),
                "wrong PE architecture",
            )
        case _:
            raise ValueError("unsupported declared binary target")
    out = Path(output)
    version = release_version()
    (wheel,) = out.glob(f"skilltest_sdk-{version}-py3-none-{platform.wheel_tag}.whl")
    with zipfile.ZipFile(wheel) as z:
        wheel_data = z.read(f"skilltest_sdk/_bin/{platform.bin}")
        require(wheel_data == data, "wheel binary differs")
        require(
            not any("_bin/skilltest-fake-provider" in name for name in z.namelist()),
            "wheel contains fake provider",
        )
        (metadata,) = [n for n in z.namelist() if n.endswith(".dist-info/METADATA")]
        headers = Parser().parsestr(z.read(metadata).decode())
        require(headers.get_all("Version", []) == [version], "wrong wheel version")
        require(
            headers.get_all("Name", []) == ["skilltest-sdk"], "wrong wheel identity"
        )

    (packed,) = out.glob("skill-test-cli-*.tgz")
    with tarfile.open(packed) as t:
        payload = t.extractfile(f"package/bin/{platform.bin}")
        if payload is None:
            raise ValueError("npm binary missing")
        npm_data = payload.read()
        require(npm_data == data, "npm binary differs")
        manifest_stream = t.extractfile("package/package.json")
        if manifest_stream is None:
            raise ValueError("npm manifest missing")
        manifest = NpmIdentity.parse(json.load(manifest_stream))
        source = NpmIdentity.parse(
            json.loads((Path(package) / "package.json").read_text())
        )
        require(manifest == source, "packed npm identity differs from source")
        require(
            manifest.name == platform.npm_package and manifest.version == version,
            "wrong npm identity/version",
        )
        expected = npm_platform(platform.target)
        require(manifest.cpu == expected.cpu, "wrong npm architecture")
        require(manifest.os == expected.os, "wrong npm OS")
        require(
            set(t.getnames())
            == {"package/package.json", f"package/bin/{platform.bin}"},
            "unexpected npm contents",
        )
    asset = asset_name(platform)
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
            archived_binary = t.extractfile(platform.bin)
            if archived_binary is None:
                raise ValueError("archive binary missing")
            archive_data = archived_binary.read()
    require(archive_data == data, "archive binary differs")
    (out / (asset + ".sha256")).write_text(f"{digest(archive.read_bytes())}  {asset}\n")
    evidence = PayloadEvidence(
        platform.target,
        version,
        digest(data),
        PayloadDigests(
            archive=digest(archive_data),
            wheel=digest(wheel_data),
            npm=digest(npm_data),
        ),
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
        zipfile.BadZipFile,
        tarfile.TarError,
    ) as error:
        sys.exit(
            f"release product: {error}; rebuild the declared target and restage its products"
        )
