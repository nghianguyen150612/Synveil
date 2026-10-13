#!/usr/bin/python3 -I
"""Register signed supplied Host runtimes into the fixed root-owned catalog.

Run as root with the independently trusted public keyrings for the three
runtime publishers. This verifies the bundle again, preserves prior cache
objects, and atomically publishes only the catalog used by the native helper.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import shutil
import stat
import tempfile

spec = importlib.util.spec_from_file_location(
    "host_runtime_producer", Path(__file__).with_name("prepare-host-runtime.py")
)
producer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(producer)

KINDS = ("server", "postgresql", "caddy")
CATALOG = Path("/etc/synveil/host-runtime-catalog.json")
CACHE = Path("/var/cache/synveil/host-runtime")


def ensure_root_directory(path, mode):
    path = Path(path)
    if not path.exists():
        path.mkdir(mode=mode)
    info = os.lstat(path)
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != 0 or info.st_mode & 0o022:
        raise ValueError("unsafe root registration directory")
    for parent in path.parents:
        if parent == Path("/"):
            break
        parent_info = os.lstat(parent)
        if not stat.S_ISDIR(parent_info.st_mode) or parent_info.st_uid != 0 or parent_info.st_mode & 0o022:
            raise ValueError("unsafe root registration parent")


def regular_json(path, limit):
    return json.loads(producer.regular(path, limit), object_pairs_hook=producer.unique_object)


def host_os():
    values = {}
    for line in Path("/etc/os-release").read_text().splitlines():
        if "=" in line:
            key, value = line.split("=", 1)
            values[key] = value.strip('"')
    identity = (values.get("ID"), values.get("VERSION_ID"))
    return {("ubuntu", "24.04"): "ubuntu-24.04", ("fedora", "42"): "fedora-42"}.get(identity)


def publish_catalog(catalog):
    ensure_root_directory(CATALOG.parent, 0o750)
    encoded = (json.dumps(catalog, sort_keys=True, indent=2) + "\n").encode()
    if CATALOG.exists() or CATALOG.is_symlink():
        old = os.lstat(CATALOG)
        if not stat.S_ISREG(old.st_mode) or old.st_uid != 0 or old.st_nlink != 1 or old.st_mode & 0o022:
            raise ValueError("existing Host catalog needs preserving repair")
        previous = producer.regular(CATALOG, 64 * 1024)
        old_catalog = json.loads(previous, object_pairs_hook=producer.unique_object)
        if (
            set(old_catalog) != {"schema_version", "qualified_os", "artifacts"}
            or old_catalog["schema_version"] != 1
            or old_catalog["qualified_os"] != catalog["qualified_os"]
            or set(old_catalog["artifacts"]) != set(KINDS)
            or any(not producer.digest_valid(value) for value in old_catalog["artifacts"].values())
        ):
            raise ValueError("existing Host catalog has an unknown identity")
        if previous == encoded:
            return
    fd, temporary = tempfile.mkstemp(prefix=".host-catalog-", dir=CATALOG.parent)
    try:
        os.fchmod(fd, 0o644)
        with os.fdopen(fd, "wb") as stream:
            stream.write(encoded)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, CATALOG)
        directory = os.open(CATALOG.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def register(args):
    if os.geteuid() != 0:
        raise ValueError("runtime registration requires explicit root administration")
    qualified_os = host_os()
    if qualified_os is None or os.uname().machine != "x86_64":
        raise ValueError("Managed Host inputs support Ubuntu 24.04/Fedora 42 x86_64")
    bundle = args.bundle.absolute()
    if any(path.is_symlink() for path in (bundle, *bundle.parents)):
        raise ValueError("redirected supplied input bundle")
    registration = regular_json(bundle / "registration.json", 64 * 1024)
    if set(registration) != {"schema_version", "qualified_os", "artifacts"} or registration["schema_version"] != 1:
        raise ValueError("invalid supplied input registration")
    if registration["qualified_os"] != qualified_os or set(registration["artifacts"]) != set(KINDS):
        raise ValueError("supplied inputs do not match this complete Host target")
    if regular_json(bundle / "catalog.json", 64 * 1024) != registration:
        raise ValueError("bundle catalog and authenticated registration differ")
    keyring_paths = {
        "server": args.server_keyring,
        "postgresql": args.postgresql_keyring,
        "caddy": args.caddy_keyring,
    }
    ensure_root_directory(CACHE.parent, 0o755)
    ensure_root_directory(CACHE, 0o755)
    staging = Path(tempfile.mkdtemp(prefix=".host-register-", dir=CACHE.parent))
    try:
        artifacts = {}
        keyrings = staging / "keyrings"
        keyrings.mkdir(mode=0o700)
        for kind in KINDS:
            identity = registration["artifacts"].get(kind)
            if not isinstance(identity, str) or not producer.digest_valid(identity):
                raise ValueError("invalid signed runtime identity")
            attestation = bundle / "attestations" / kind
            manifest = attestation / "manifest.json"
            signature = attestation / "manifest.sig"
            source = bundle / "cache" / identity
            keyring = keyrings / (kind + ".gpg")
            key_bytes = producer.regular(keyring_paths[kind], 16 * 1024 * 1024)
            with keyring.open("xb") as stream:
                stream.write(key_bytes)
                stream.flush()
                os.fsync(stream.fileno())
            manifest_bytes = producer.regular(manifest, 2 * 1024 * 1024)
            if producer.digest(manifest_bytes) != identity:
                raise ValueError("signed manifest identity mismatch")
            manifest_info = json.loads(
                manifest_bytes, object_pairs_hook=producer.unique_object
            )
            if (
                manifest_info.get("kind") != kind
                or manifest_info.get("qualified_os") != qualified_os
            ):
                raise ValueError("signed runtime kind or target platform mismatch")
            result = type("Input", (), {
                "manifest": manifest,
                "signature": signature,
                "trusted_keyring": keyring,
                "input": source,
                "output": CACHE,
            })()
            producer.publish(result)
            artifacts[kind] = identity
        catalog = {"schema_version": 1, "qualified_os": qualified_os, "artifacts": artifacts}
        publish_catalog(catalog)
    finally:
        shutil.rmtree(staging)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", type=Path, required=True)
    parser.add_argument("--server-keyring", type=Path, required=True)
    parser.add_argument("--postgresql-keyring", type=Path, required=True)
    parser.add_argument("--caddy-keyring", type=Path, required=True)
    try:
        register(parser.parse_args())
    except (ValueError, OSError, KeyError, TypeError) as error:
        parser.exit(2, "Host runtime registration blocked: " + type(error).__name__ + "\n")
