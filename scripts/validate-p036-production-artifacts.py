#!/usr/bin/env python3
"""Verify P036's signed, exact production artifact inventory.

No artifact identity is inferred from a filename. Missing payloads, signer
trust, signatures, provenance, or license bytes are external-input blockers.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
SCHEMA = ROOT / "acceptance/p036/production-artifacts.schema.json"
COMPONENTS = ("desktop", "server", "postgresql17", "edge")
PLATFORMS = ("fedora-42", "ubuntu-24.04")
EXPECTED_LICENSES = {
    "desktop": "MIT",
    "server": "MIT",
    "postgresql17": "PostgreSQL",
    "edge": "Apache-2.0",
}
TOP_FIELDS = {"schema_version", "product", "product_version", "source_commit", "artifacts"}
ENTRY_FIELDS = {
    "component", "version", "source_revision", "platform", "architecture",
    "artifact", "provenance", "license",
}
ARTIFACT_FIELDS = {"path", "format", "size_bytes", "sha256", "signature_path", "payload_root"}
PROVENANCE_FIELDS = {"path", "sha256", "signature_path", "source_url", "source_revision"}
LICENSE_FIELDS = {"spdx_id", "path", "sha256", "notice_path", "notice_sha256"}
HEX64 = re.compile(r"[0-9a-f]{64}\Z")
HEX40 = re.compile(r"[0-9a-f]{40}\Z")
FINGERPRINT = re.compile(r"[0-9A-F]{40,64}\Z")
VERSION = re.compile(r"[A-Za-z0-9][A-Za-z0-9.+_-]{2,127}\Z")


class InventoryError(ValueError):
    pass


class ExternalInputError(InventoryError):
    pass


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise InventoryError("duplicate JSON key")
        result[key] = value
    return result


def fields(value, expected: set[str], where: str) -> None:
    if not isinstance(value, dict):
        raise InventoryError(f"{where}: object required")
    missing = sorted(expected - value.keys())
    extra = sorted(value.keys() - expected)
    if missing:
        raise InventoryError(f"{where}: missing {missing[0]}")
    if extra:
        raise InventoryError(f"{where}: unknown {extra[0]}")


def relative_path(value: object, where: str) -> PurePosixPath:
    if not isinstance(value, str) or not 1 <= len(value) <= 512 or "\\" in value:
        raise InventoryError(f"{where}: invalid relative path")
    if any(part in ("", ".", "..") for part in value.split("/")):
        raise InventoryError(f"{where}: redirected path")
    path = PurePosixPath(value)
    if path.is_absolute() or any(part in ("", ".", "..") for part in path.parts):
        raise InventoryError(f"{where}: redirected path")
    if any(not re.fullmatch(r"[A-Za-z0-9._+-]+", part) for part in path.parts):
        raise InventoryError(f"{where}: invalid path component")
    return path


def regular_file(path: Path, limit: int = 2 * 1024 * 1024 * 1024) -> os.stat_result:
    try:
        before = path.lstat()
    except OSError as error:
        raise ExternalInputError("required production input is missing") from error
    if not stat.S_ISREG(before.st_mode) or before.st_nlink != 1 or before.st_size > limit:
        raise InventoryError("production input is not a bounded single-link regular file")
    try:
        fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK)
    except OSError as error:
        raise InventoryError("production input changed or became a redirected file") from error
    try:
        info = os.fstat(fd)
        if (not stat.S_ISREG(info.st_mode) or info.st_nlink != 1 or info.st_size > limit
                or (info.st_dev, info.st_ino) != (before.st_dev, before.st_ino)):
            raise InventoryError("production input changed during identity validation")
        return info
    finally:
        os.close(fd)


def read_regular_bytes(path: Path, limit: int) -> bytes:
    info = regular_file(path, limit)
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK)
    try:
        opened = os.fstat(fd)
        if (opened.st_dev, opened.st_ino) != (info.st_dev, info.st_ino):
            raise InventoryError("production input changed during read")
        with os.fdopen(fd, "rb", closefd=False) as stream:
            data = stream.read(limit + 1)
    finally:
        os.close(fd)
    if len(data) > limit:
        raise InventoryError("production input exceeded its read bound")
    return data


def confined_file(root: Path, value: object, where: str, limit: int = 2 * 1024 * 1024 * 1024) -> Path:
    relative = relative_path(value, where)
    root = root.resolve(strict=True)
    path = root.joinpath(*relative.parts)
    current = root
    for part in relative.parts:
        current = current / part
        try:
            info = current.lstat()
        except OSError as error:
            raise ExternalInputError(f"{where}: referenced file is missing") from error
        if stat.S_ISLNK(info.st_mode):
            raise InventoryError(f"{where}: symlink path rejected")
    regular_file(path, limit)
    if root not in path.resolve(strict=True).parents:
        raise InventoryError(f"{where}: path escapes artifact root")
    return path


def digest_file(path: Path) -> tuple[int, str]:
    info = regular_file(path)
    digest = hashlib.sha256()
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK)
    try:
        opened = os.fstat(fd)
        if (opened.st_dev, opened.st_ino) != (info.st_dev, info.st_ino):
            raise InventoryError("production input changed during digest calculation")
        with os.fdopen(fd, "rb", closefd=False) as stream:
            for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                digest.update(chunk)
    finally:
        os.close(fd)
    return info.st_size, digest.hexdigest()


def check_digest(path: Path, size: object, expected: object, where: str) -> None:
    if type(size) is not int or size <= 0 or not isinstance(expected, str) or not HEX64.fullmatch(expected):
        raise InventoryError(f"{where}: invalid size or SHA-256")
    actual_size, actual_hash = digest_file(path)
    if actual_size != size:
        raise InventoryError(f"{where}: size mismatch")
    if actual_hash != expected:
        raise InventoryError(f"{where}: independently calculated SHA-256 mismatch")


def load_json(path: Path, limit: int = 2 * 1024 * 1024):
    try:
        raw = read_regular_bytes(path, limit)
        return raw, json.loads(raw, object_pairs_hook=unique_object)
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise InventoryError("invalid production JSON input") from error


def validate_structure(data: object, current_commit: str, selected_platform: str | None) -> dict:
    validate_json_schema(data)
    fields(data, TOP_FIELDS, "inventory")
    if not isinstance(current_commit, str) or not HEX40.fullmatch(current_commit):
        raise InventoryError("checked-out source commit is not a full Git SHA")
    if type(data["schema_version"]) is not int or data["schema_version"] != 1:
        raise InventoryError("unsupported P036 inventory schema version")
    if data["product"] != "Synveil":
        raise InventoryError("inventory product mismatch")
    if not isinstance(data["product_version"], str) or not re.fullmatch(
        r"[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.-]+)?", data["product_version"]
    ):
        raise InventoryError("invalid product version")
    try:
        workspace_version = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))[
            "workspace"]["package"]["version"]
    except (OSError, UnicodeError, KeyError, tomllib.TOMLDecodeError) as error:
        raise InventoryError("cannot determine the source product version") from error
    if data["product_version"] != workspace_version:
        raise InventoryError("inventory product version differs from Cargo workspace version")
    if not isinstance(data["source_commit"], str) or not HEX40.fullmatch(data["source_commit"]):
        raise InventoryError("source_commit must be a full lowercase Git SHA")
    if not isinstance(data["artifacts"], list) or len(data["artifacts"]) != 8:
        raise InventoryError("inventory must contain four components for both qualified platforms")

    seen = set()
    previous = None
    for index, entry in enumerate(data["artifacts"]):
        where = f"artifacts[{index}]"
        fields(entry, ENTRY_FIELDS, where)
        component = entry["component"]
        platform = entry["platform"]
        if component not in COMPONENTS or platform not in PLATFORMS:
            raise InventoryError(f"{where}: unsupported component or platform")
        pair = (platform, component)
        if pair in seen:
            raise InventoryError(f"{where}: duplicate component/platform entry")
        if previous is not None and pair < previous:
            raise InventoryError("artifact entries must be sorted by platform and component")
        seen.add(pair)
        previous = pair
        if entry["architecture"] != "x86_64":
            raise InventoryError(f"{where}: only x86_64 is qualified")
        if not isinstance(entry["version"], str) or not VERSION.fullmatch(entry["version"]):
            raise InventoryError(f"{where}: exact version required")
        lowered_version = entry["version"].lower()
        if any(token in lowered_version for token in ("latest", "unknown", "placeholder", "unverified")) or lowered_version == "0.0.0":
            raise InventoryError(f"{where}: placeholder version rejected")
        if component in {"desktop", "server"} and entry["version"] != data["product_version"]:
            raise InventoryError(f"{where}: product version mismatch")
        if component in {"desktop", "server"} and entry["source_revision"] != data["source_commit"]:
            raise InventoryError(f"{where}: source revision does not match inventory source commit")
        if not isinstance(entry["source_revision"], str) or not HEX40.fullmatch(entry["source_revision"]):
            raise InventoryError(f"{where}: full source revision required")
        artifact = entry["artifact"]
        fields(artifact, ARTIFACT_FIELDS if component != "desktop" else ARTIFACT_FIELDS - {"payload_root"}, f"{where}.artifact")
        expected_format = {"desktop": "deb" if platform == "ubuntu-24.04" else "rpm"}
        expected_format = expected_format.get(component, "runtime_manifest_v2")
        if artifact["format"] != expected_format:
            raise InventoryError(f"{where}: artifact format/platform mismatch")
        if component != "desktop" and "payload_root" not in artifact:
            raise InventoryError(f"{where}: runtime payload_root missing")
        if component == "desktop" and "payload_root" in artifact:
            raise InventoryError(f"{where}: desktop package cannot name a runtime payload root")
        if not isinstance(artifact["signature_path"], str):
            raise InventoryError(f"{where}: detached artifact signature required")

        provenance = entry["provenance"]
        fields(provenance, PROVENANCE_FIELDS, f"{where}.provenance")
        if not isinstance(provenance["source_url"], str) or not provenance["source_url"].startswith("https://"):
            raise InventoryError(f"{where}: authenticated HTTPS source required")
        if provenance["source_revision"] != entry["source_revision"]:
            raise InventoryError(f"{where}: provenance source revision mismatch")

        license_info = entry["license"]
        fields(license_info, LICENSE_FIELDS, f"{where}.license")
        if license_info["spdx_id"] != EXPECTED_LICENSES[component]:
            raise InventoryError(f"{where}: unapproved license identifier")

    if seen != {(platform, component) for platform in PLATFORMS for component in COMPONENTS}:
        raise InventoryError("inventory component/platform matrix is incomplete")
    if selected_platform is not None and selected_platform not in PLATFORMS:
        raise InventoryError("unsupported selected platform")
    return data


def validate_json_schema(data: object) -> None:
    try:
        import jsonschema
    except ImportError as error:
        raise ExternalInputError("pinned jsonschema validator dependency is unavailable") from error
    try:
        _, schema = load_json(SCHEMA, 256 * 1024)
        jsonschema.Draft202012Validator.check_schema(schema)
        errors = sorted(
            jsonschema.Draft202012Validator(schema).iter_errors(data),
            key=lambda error: tuple(str(part) for part in error.absolute_path),
        )
    except (OSError, jsonschema.SchemaError) as error:
        raise InventoryError("P036 JSON schema is invalid or unavailable") from error
    if errors:
        location = ".".join(str(part) for part in errors[0].absolute_path) or "inventory"
        raise InventoryError(f"{location}: does not satisfy the versioned P036 JSON schema")


def copy_keyring(path_value: str, directory: Path) -> Path:
    path = Path(path_value)
    if not path.is_absolute():
        raise ExternalInputError("trusted publisher keyring path is not configured")
    keyring_bytes = read_regular_bytes(path, 16 * 1024 * 1024)
    keyring_info = path.lstat()
    if keyring_info.st_uid not in {0, os.geteuid()} or keyring_info.st_mode & 0o022:
        raise ExternalInputError("trusted publisher keyring must be root/runner-owned and not writable by group or others")
    target = directory / ("keyring-" + hashlib.sha256(str(path).encode()).hexdigest()[:12] + ".gpg")
    target.write_bytes(keyring_bytes)
    target.chmod(0o600)
    return target


def verify_signature(keyring: Path, signature: Path, signed_file: Path, expected: str) -> None:
    if not isinstance(expected, str) or not FINGERPRINT.fullmatch(expected):
        raise ExternalInputError("trusted signer fingerprint is not configured")
    gpgv = shutil.which("gpgv")
    if gpgv is None:
        raise ExternalInputError("gpgv is unavailable")
    try:
        result = subprocess.run(
            [gpgv, "--status-fd=1", "--keyring", str(keyring), str(signature), str(signed_file)],
            stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
            timeout=30, check=True, env={"PATH": "/usr/bin:/bin", "LC_ALL": "C", "LANG": "C"},
        )
    except (OSError, subprocess.SubprocessError) as error:
        raise InventoryError("artifact or provenance signature verification failed") from error
    statuses = result.stdout.decode("ascii", "replace").splitlines()
    valid = [line.split() for line in statuses if line.startswith("[GNUPG:] VALIDSIG ")]
    if not valid or valid[0][2].upper() != expected:
        raise InventoryError("signature does not match the independently trusted signer fingerprint")


def verify_provenance(root: Path, entry: dict, keyring: Path, expected_fingerprint: str) -> None:
    info = entry["provenance"]
    path = confined_file(root, info["path"], "provenance")
    signature = confined_file(root, info["signature_path"], "provenance signature", 16 * 1024 * 1024)
    size, actual = digest_file(path)
    if not size or actual != info["sha256"] or not HEX64.fullmatch(info["sha256"]):
        raise InventoryError("provenance digest mismatch")
    verify_signature(keyring, signature, path, expected_fingerprint)
    _, provenance = load_json(path, 1024 * 1024)
    required = {"schema_version", "component", "version", "source_revision", "source_url", "platform", "architecture"}
    if not isinstance(provenance, dict) or set(provenance) != required:
        raise InventoryError("provenance fields are incomplete or unknown")
    source = urlsplit(provenance["source_url"])
    if (provenance["schema_version"] != 1 or provenance["component"] != entry["component"]
            or provenance["version"] != entry["version"]
            or provenance["source_revision"] != entry["source_revision"]
            or provenance["source_url"] != info["source_url"]
            or provenance["platform"] != entry["platform"]
            or provenance["architecture"] != entry["architecture"]
            or source.scheme != "https" or not source.hostname
            or source.username is not None or source.password is not None or source.fragment):
        raise InventoryError("signed provenance does not match inventory identity")


def verify_license(root: Path, entry: dict) -> None:
    license_info = entry["license"]
    path = confined_file(root, license_info["path"], "license")
    notice = confined_file(root, license_info["notice_path"], "license notice")
    if digest_file(path)[1] != license_info["sha256"] or not HEX64.fullmatch(license_info["sha256"]):
        raise InventoryError("license file digest mismatch")
    if digest_file(notice)[1] != license_info["notice_sha256"] or not HEX64.fullmatch(license_info["notice_sha256"]):
        raise InventoryError("license notice digest mismatch")


def runtime_manifest(entry: dict, root: Path, keyring: Path, expected_fingerprint: str,
                     source_commit: str) -> dict:
    artifact = entry["artifact"]
    manifest_path = confined_file(root, artifact["path"], "runtime manifest", 2 * 1024 * 1024)
    signature = confined_file(root, artifact["signature_path"], "runtime signature", 16 * 1024 * 1024)
    check_digest(manifest_path, artifact["size_bytes"], artifact["sha256"], "runtime manifest")
    verify_signature(keyring, signature, manifest_path, expected_fingerprint)
    _, manifest = load_json(manifest_path)
    expected_kind = {"server": "server", "postgresql17": "postgresql", "edge": "caddy"}[entry["component"]]
    if (manifest.get("schema_version") != 2 or manifest.get("kind") != expected_kind
            or manifest.get("version") != entry["version"]
            or manifest.get("packaging_revision") != source_commit
            or manifest.get("upstream_revision") != entry["source_revision"]
            or manifest.get("upstream_url") != entry["provenance"]["source_url"]
            or manifest.get("qualified_os") != entry["platform"]
            or manifest.get("architecture") != entry["architecture"]):
        raise InventoryError("signed runtime manifest identity differs from P036 inventory")
    payload = confined_dir(root, artifact["payload_root"], "runtime payload")
    listed = manifest.get("files")
    if not isinstance(listed, list) or not listed:
        raise InventoryError("signed runtime manifest has no closed file inventory")
    expected = set()
    for item in listed:
        if not isinstance(item, dict) or set(item) != {"path", "sha256", "bytes", "executable"}:
            raise InventoryError("invalid runtime inventory member")
        name = str(relative_path(item["path"], "runtime member"))
        if name in expected:
            raise InventoryError("duplicate runtime inventory member")
        expected.add(name)
        member = confined_file(payload, name, "runtime member")
        check_digest(member, item["bytes"], item["sha256"], "runtime member")
    actual = set()
    for directory, dirs, files in os.walk(payload, followlinks=False):
        for child in dirs:
            if (Path(directory) / child).is_symlink():
                raise InventoryError("runtime payload contains a directory symlink")
        for child in files:
            relative = (Path(directory) / child).relative_to(payload).as_posix()
            member = Path(directory) / child
            regular_file(member)
            if relative != "manifest.json":
                actual.add(relative)
            elif member.read_bytes() != manifest_path.read_bytes():
                raise InventoryError("runtime payload manifest differs from signed manifest")
    if actual != expected:
        raise InventoryError("runtime payload closed inventory mismatch")
    if manifest.get("license") != {
        "spdx_id": entry["license"]["spdx_id"],
        "license_file": "LICENSE",
        "license_sha256": entry["license"]["sha256"],
        "notice_file": "NOTICE",
        "notice_sha256": entry["license"]["notice_sha256"],
    }:
        raise InventoryError("signed runtime license identity differs from inventory")
    if entry["provenance"]["path"] != str(PurePosixPath(artifact["payload_root"]) / "PROVENANCE"):
        raise InventoryError("runtime provenance path does not match the signed payload")
    if entry["license"]["path"] != str(PurePosixPath(artifact["payload_root"]) / "LICENSE"):
        raise InventoryError("runtime license path does not match the signed payload")
    if entry["license"]["notice_path"] != str(PurePosixPath(artifact["payload_root"]) / "NOTICE"):
        raise InventoryError("runtime notice path does not match the signed payload")
    if manifest.get("provenance_sha256") != entry["provenance"]["sha256"]:
        raise InventoryError("signed runtime provenance digest differs from inventory")
    return manifest


def confined_dir(root: Path, value: object, where: str) -> Path:
    relative = relative_path(value, where)
    root = root.resolve(strict=True)
    path = root.joinpath(*relative.parts)
    current = root
    for part in relative.parts:
        current = current / part
        try:
            info = current.lstat()
        except OSError as error:
            raise ExternalInputError(f"{where}: directory missing") from error
        if stat.S_ISLNK(info.st_mode):
            raise InventoryError(f"{where}: symlink directory rejected")
    if not path.is_dir() or root not in path.resolve(strict=True).parents:
        raise InventoryError(f"{where}: invalid directory")
    return path


def verify_desktop_package(entry: dict, root: Path) -> None:
    artifact = entry["artifact"]
    package = confined_file(root, artifact["path"], "desktop package")
    signature = confined_file(root, artifact["signature_path"], "desktop package signature", 16 * 1024 * 1024)
    check_digest(package, artifact["size_bytes"], artifact["sha256"], "desktop package")
    if artifact["format"] == "deb":
        tool = shutil.which("dpkg-deb")
        if tool is None:
            raise ExternalInputError("dpkg-deb is unavailable for package identity inspection")
        try:
            result = subprocess.run(
                [tool, "--field", str(package), "Version", "Architecture"],
                check=True, timeout=20, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                stderr=subprocess.DEVNULL, env={"PATH": "/usr/bin:/bin", "LC_ALL": "C"},
            )
            values = result.stdout.decode("utf-8", "strict").splitlines()
        except (OSError, UnicodeError, subprocess.SubprocessError) as error:
            raise InventoryError("DEB package metadata inspection failed") from error
        expected_arch = "amd64"
        package_version = values[0] if len(values) >= 1 else ""
        package_arch = values[1] if len(values) >= 2 else ""
        if package_version.split("-", 1)[0] != entry["version"] or package_arch != expected_arch:
            raise InventoryError("DEB package version or architecture mismatch")
    else:
        tool = shutil.which("rpm")
        if tool is None:
            raise ExternalInputError("rpm is unavailable for package identity inspection")
        try:
            result = subprocess.run(
                [tool, "-qp", "--qf", "%{VERSION}\\n%{ARCH}\\n%{LICENSE}\\n", str(package)],
                check=True, timeout=20, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                stderr=subprocess.DEVNULL, env={"PATH": "/usr/bin:/bin", "LC_ALL": "C"},
            )
            values = result.stdout.decode("utf-8", "strict").splitlines()
        except (OSError, UnicodeError, subprocess.SubprocessError) as error:
            raise InventoryError("RPM package metadata inspection failed") from error
        if (len(values) < 3 or values[0] != entry["version"] or values[1] != "x86_64"
                or values[2] != EXPECTED_LICENSES["desktop"]):
            raise InventoryError("RPM package version, architecture, or license metadata mismatch")


def load_current_commit() -> str:
    try:
        result = subprocess.run(
            ["git", "-C", str(ROOT), "rev-parse", "HEAD"],
            check=True, timeout=10, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL, env={"PATH": "/usr/bin:/bin", "LC_ALL": "C"},
        )
        return result.stdout.decode("ascii").strip()
    except (OSError, UnicodeError, subprocess.SubprocessError) as error:
        raise InventoryError("cannot bind inventory to the checked-out source") from error


def verify_source_commit(source_commit: str, current_commit: str) -> None:
    """Bind artifacts to an immutable source commit and reject stale binaries.

    The inventory may be committed after the source it describes, but only
    P036 inventory metadata may have changed since that source revision.
    """
    try:
        ancestor = subprocess.run(
            ["git", "-C", str(ROOT), "merge-base", "--is-ancestor", source_commit, current_commit],
            check=False, timeout=10, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL, env={"PATH": "/usr/bin:/bin", "LC_ALL": "C"},
        )
        if ancestor.returncode != 0:
            raise InventoryError("inventory source_commit is not an ancestor of the checked-out source")
        changed = subprocess.run(
            ["git", "-C", str(ROOT), "diff", "--name-only", source_commit, current_commit, "--", "."],
            check=True, timeout=20, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL, env={"PATH": "/usr/bin:/bin", "LC_ALL": "C"},
        ).stdout.decode("utf-8", "strict").splitlines()
    except (OSError, UnicodeError, subprocess.SubprocessError) as error:
        raise InventoryError("cannot verify inventory source ancestry") from error
    allowed = {
        "acceptance/p036/production-artifacts.json",
        "acceptance/p036/production-artifacts.json.sig",
        "docs/v0.2/RELEASE_HARDENING_R02_MANIFEST.md",
        "docs/v0.2/RELEASE_HARDENING_TRACKER.md",
    }
    if set(changed) - allowed:
        raise InventoryError("production artifacts are stale against changed source files")


def validate_files(data: dict, artifact_root: Path, selected_platform: str | None,
                   trust_keyring_path: str, inventory_path: Path,
                   inventory_signature: Path, signers: dict[str, str]) -> list[dict]:
    artifact_root = artifact_root.resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="synveil-p036-trust-") as temporary:
        trust_dir = Path(temporary)
        keyring = copy_keyring(trust_keyring_path, trust_dir)
        expected_inventory = os.environ.get("SYNVEIL_P036_INVENTORY_FINGERPRINT", "")
        regular_file(inventory_path, 2 * 1024 * 1024)
        regular_file(inventory_signature, 16 * 1024 * 1024)
        verify_signature(keyring, inventory_signature, inventory_path, expected_inventory)

        selected = [entry for entry in data["artifacts"] if selected_platform is None or entry["platform"] == selected_platform]
        normalized = []
        for entry in selected:
            component = entry["component"]
            fingerprint = signers.get(component, "")
            if not FINGERPRINT.fullmatch(fingerprint):
                raise ExternalInputError(f"trusted {component} signer fingerprint is not configured")
            entry_signature = confined_file(artifact_root, entry["artifact"]["signature_path"], f"{component} signature", 16 * 1024 * 1024)
            artifact_path = confined_file(artifact_root, entry["artifact"]["path"], f"{component} artifact")
            verify_signature(keyring, entry_signature, artifact_path, fingerprint)
            verify_provenance(artifact_root, entry, keyring, fingerprint)
            verify_license(artifact_root, entry)
            if component == "desktop":
                verify_desktop_package(entry, artifact_root)
            else:
                manifest = runtime_manifest(entry, artifact_root, keyring, fingerprint,
                                            data["source_commit"])
                # Reuse the production host-input verifier, which rechecks the
                # detached signature, closed file set, byte digests, ELF target,
                # license metadata, provenance and executable-reported version.
                output = trust_dir / f"publish-{component}"
                command = [
                    sys.executable, str(ROOT / "deploy/server/prepare-host-runtime.py"),
                    "--manifest", str(artifact_path),
                    "--signature", str(entry_signature),
                    "--trusted-keyring", str(keyring),
                    "--input", str(confined_dir(artifact_root, entry["artifact"]["payload_root"], "runtime payload")),
                    "--output", str(output),
                ]
                try:
                    subprocess.run(
                        command, check=True, timeout=240, stdin=subprocess.DEVNULL,
                        stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
                        env={"PATH": "/usr/bin:/bin", "LC_ALL": "C", "LANG": "C"},
                    )
                except (OSError, subprocess.SubprocessError) as error:
                    raise InventoryError("production Host runtime verification failed") from error
                finally:
                    shutil.rmtree(output, ignore_errors=True)
            normalized.append({
                "component": component,
                "version": entry["version"],
                "platform": entry["platform"],
                "architecture": entry["architecture"],
                "artifact_path": str(artifact_path),
                "artifact_format": entry["artifact"]["format"],
                "artifact_sha256": entry["artifact"]["sha256"],
                "artifact_signature_path": str(entry_signature),
                "payload_root": entry["artifact"].get("payload_root"),
            })
        return normalized


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inventory", type=Path, default=ROOT / "acceptance/p036/production-artifacts.json")
    parser.add_argument("--inventory-signature", type=Path)
    parser.add_argument("--artifact-root", type=Path, default=ROOT / "acceptance/p036/artifacts")
    parser.add_argument("--platform", choices=PLATFORMS)
    parser.add_argument("--print-json", action="store_true")
    args = parser.parse_args()
    try:
        if not args.inventory.is_file():
            raise ExternalInputError("production-artifacts.json has not been supplied")
        if not args.artifact_root.is_dir():
            raise ExternalInputError("P036 production artifact files have not been supplied")
        _, data = load_json(args.inventory)
        current_commit = load_current_commit()
        validated = validate_structure(data, current_commit, args.platform)
        verify_source_commit(validated["source_commit"], current_commit)
        inventory_signature = args.inventory_signature or Path(str(args.inventory) + ".sig")
        keyring = os.environ.get("SYNVEIL_P036_TRUSTED_KEYRING", "")
        if not keyring:
            raise ExternalInputError("independently trusted P036 publisher keyring is not configured")
        signers = {component: os.environ.get(f"SYNVEIL_P036_{component.upper()}_FINGERPRINT", "") for component in COMPONENTS}
        normalized = validate_files(validated, args.artifact_root, args.platform, keyring,
                                    args.inventory.resolve(strict=True), inventory_signature, signers)
    except ExternalInputError as error:
        print(f"BLOCKED_BY_EXTERNAL_INPUT: {error}", file=sys.stderr)
        return 2
    except (InventoryError, OSError, KeyError, TypeError, ValueError) as error:
        print(f"P036 artifact inventory rejected: {error}", file=sys.stderr)
        return 1
    if args.print_json:
        print(json.dumps({"schema_version": 1, "source_commit": validated["source_commit"], "artifacts": normalized}, sort_keys=True))
    else:
        print(f"P036 production artifact inventory verified: {len(normalized)} artifacts for {args.platform or 'selected platforms'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
