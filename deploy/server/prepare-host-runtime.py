#!/usr/bin/env python3
"""Verify supplied, signed, pinned runtime inputs; publish preserving bundles.

This command does not fetch mutable packages, manufacture provenance, sign, or
register a root trust anchor. Native package assembly installs the resulting
catalog only after operator review. A missing input is a blocker, not a fixture.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import tempfile

FIELDS = {"schema_version", "kind", "version", "packaging_revision", "platform",
          "architecture", "qualified_os", "compatibility", "upstream_url",
          "upstream_revision", "provenance_sha256", "files", "usage"}
REQUIRED = {
    "postgresql": {"bin/postgres", "bin/initdb", "bin/pg_ctl", "bin/pg_controldata", "bin/psql"},
    "caddy": {"bin/caddy"},
    "server": {"bin/synveil-api", "bin/synveil-worker", "bin/synveil-server-migrate"},
}

def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate JSON key")
        result[key] = value
    return result

def regular(path, limit):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    with os.fdopen(fd, "rb") as stream:
        info = os.fstat(stream.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1 or info.st_size > limit:
            raise ValueError("invalid regular input")
        data = stream.read(limit + 1)
        if len(data) > limit:
            raise ValueError("input exceeds bound")
        return data

def digest(data):
    return hashlib.sha256(data).hexdigest()

def publish(args):
    raw = regular(args.manifest, 2 * 1024 * 1024)
    manifest = json.loads(raw, object_pairs_hook=unique_object)
    if set(manifest) != FIELDS or manifest["schema_version"] != 1:
        raise ValueError("unsupported manifest schema")
    if (manifest["platform"], manifest["architecture"], manifest["compatibility"]) != (
            "linux", "x86_64", "synveil-desktop-server-v0.2"):
        raise ValueError("unsupported compatibility")
    if manifest["qualified_os"] not in ("ubuntu-24.04", "fedora-42"):
        raise ValueError("unsupported Host platform")
    if manifest["usage"] != "managed_host_production_input":
        raise ValueError("CI fixtures are not managed Host inputs")
    kind = manifest["kind"]
    if kind not in REQUIRED or not manifest["version"] or "latest" in manifest["version"]:
        raise ValueError("exact runtime version required")
    if kind == "postgresql" and not manifest["version"].startswith("17."):
        raise ValueError("PostgreSQL 17 required")
    if not manifest["upstream_url"].startswith("https://") or not manifest["upstream_revision"]:
        raise ValueError("authenticated upstream provenance required")
    if len(manifest["packaging_revision"]) != 40 or any(c not in "0123456789abcdef" for c in manifest["packaging_revision"]):
        raise ValueError("exact packaging revision required")
    # gpgv is the mature signature verifier. The trusted keyring must be supplied
    # by the operator/build policy; no key is fetched from the input itself. Copy
    # all three public inputs into a private directory so verification uses the
    # exact bytes already read without following links or racing a mutable path.
    signature_bytes = regular(args.signature, 16 * 1024 * 1024)
    keyring_bytes = regular(args.trusted_keyring, 16 * 1024 * 1024)
    verify_dir = Path(tempfile.mkdtemp(prefix=".host-signature-"))
    try:
        verified_paths = {}
        for name, data in (("manifest", raw), ("signature", signature_bytes), ("keyring", keyring_bytes)):
            target = verify_dir / name
            with target.open("xb") as stream:
                stream.write(data)
                stream.flush()
                os.fsync(stream.fileno())
            target.chmod(0o600)
            verified_paths[name] = target
        subprocess.run(["/usr/bin/gpgv", "--keyring", str(verified_paths["keyring"]),
                        str(verified_paths["signature"]), str(verified_paths["manifest"])],
                       check=True, timeout=30, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                       env={"PATH": "/usr/bin:/bin", "LC_ALL": "C"})
    finally:
        shutil.rmtree(verify_dir)
    inventory = {}
    for item in manifest["files"]:
        if set(item) != {"path", "sha256", "bytes", "executable"}:
            raise ValueError("unknown inventory field")
        name = item["path"]
        if (not name or name.startswith("/") or "\\" in name or
                name == "manifest.json" or
                any(part in ("", ".", "..") for part in name.split("/")) or name in inventory):
            raise ValueError("unsafe inventory path")
        if not isinstance(item["executable"], bool) or not 0 <= item["bytes"] <= 512 * 1024 * 1024:
            raise ValueError("invalid inventory bounds")
        inventory[name] = item
    if not 0 < len(inventory) <= 8192 or not (REQUIRED[kind] | {"LICENSE", "NOTICE", "PROVENANCE"}) <= inventory.keys():
        raise ValueError("incomplete runtime inventory")
    if any(inventory[name]["bytes"] == 0 or not inventory[name]["executable"] for name in REQUIRED[kind]):
        raise ValueError("required runtime program is empty or not executable")
    if any(inventory[name]["bytes"] == 0 for name in ("LICENSE", "NOTICE", "PROVENANCE")):
        raise ValueError("license and provenance contents are required")
    if sum(item["bytes"] for item in inventory.values()) > 2 * 1024 * 1024 * 1024:
        raise ValueError("runtime total bytes exceed bound")
    actual = set()
    for at, directories, files in os.walk(args.input, followlinks=False):
        for name in directories:
            if Path(at, name).is_symlink():
                raise ValueError("runtime directory symlink")
        for name in files:
            actual.add(Path(at, name).relative_to(args.input).as_posix())
    # An assembled bundle carries the authenticated manifest beside the closed
    # payload tree. Admit that one metadata file only when its bytes are the
    # exact signed manifest read above; all runtime payload extras still fail.
    if "manifest.json" in actual:
        if regular(args.input / "manifest.json", 2 * 1024 * 1024) != raw:
            raise ValueError("assembled manifest differs from signed input")
        actual.remove("manifest.json")
    if actual != inventory.keys():
        raise ValueError("closed inventory mismatch")
    verified = {}
    for name, item in inventory.items():
        data = regular(args.input / name, item["bytes"])
        if len(data) != item["bytes"] or digest(data) != item["sha256"]:
            raise ValueError("runtime checksum mismatch")
        if item["executable"] and name.startswith("bin/"):
            header = subprocess.run(["/usr/bin/readelf", "--file-header", str(args.input / name)],
                                    check=True, timeout=10, capture_output=True,
                                    env={"PATH": "/usr/bin:/bin", "LC_ALL": "C"}).stdout
            if b"ELF64" not in header or b"Advanced Micro Devices X86-64" not in header:
                raise ValueError("runtime executable architecture mismatch")
        verified[name] = data
    if inventory["PROVENANCE"]["sha256"] != manifest["provenance_sha256"]:
        raise ValueError("provenance checksum mismatch")
    identity = digest(raw)
    if any(path.is_symlink() for path in (args.output, *args.output.absolute().parents)):
        raise ValueError("redirected runtime publication path")
    args.output.mkdir(parents=True, exist_ok=True)
    destination = args.output / identity
    if destination.exists():
        actual = set()
        for at, directories, files in os.walk(destination, followlinks=False):
            if any(Path(at, name).is_symlink() for name in directories):
                raise ValueError("existing publication contains directory symlink")
            actual.update(Path(at, name).relative_to(destination).as_posix() for name in files)
        if actual != set(inventory) | {"manifest.json"}:
            raise ValueError("existing publication inventory mismatch")
        if regular(destination / "manifest.json", len(raw)) != raw:
            raise ValueError("existing publication conflict")
        for name, data in verified.items():
            if regular(destination / name, len(data)) != data:
                raise ValueError("existing publication conflict")
            expected_mode = 0o755 if inventory[name]["executable"] else 0o644
            if stat.S_IMODE((destination / name).stat().st_mode) != expected_mode:
                raise ValueError("existing publication mode conflict")
        print(json.dumps({"kind": kind, "manifest_sha256": identity, "reused": True}))
        return
    temporary = Path(tempfile.mkdtemp(prefix=".synveil-runtime-", dir=args.output))
    try:
        for name, data in {**verified, "manifest.json": raw}.items():
            target = temporary / name
            target.parent.mkdir(parents=True, exist_ok=True)
            with target.open("xb") as stream:
                stream.write(data)
                stream.flush()
                os.fsync(stream.fileno())
            target.chmod(0o755 if inventory.get(name, {}).get("executable") else 0o644)
        for at, _, _ in os.walk(temporary, topdown=False):
            Path(at).chmod(0o755)
            fd = os.open(at, os.O_RDONLY | os.O_DIRECTORY)
            try:
                os.fsync(fd)
            finally:
                os.close(fd)
        os.rename(temporary, destination)
        fd = os.open(args.output, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(fd)
        finally:
            os.close(fd)
    finally:
        if temporary.exists():
            shutil.rmtree(temporary)
    print(json.dumps({"kind": kind, "manifest_sha256": identity, "reused": False}))

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("manifest", "signature", "trusted-keyring", "input", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    try:
        publish(parser.parse_args())
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        parser.exit(2, "verified runtime input unavailable or invalid: " + type(error).__name__ + "\n")
