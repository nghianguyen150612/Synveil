#!/usr/bin/env python3
"""Assemble a closed Host input bundle from authenticated supplied bytes.

Output contains catalog.json, cache/<manifest-sha>/, and the signed manifests
needed for a root registrar to verify supplied keyrings again. It contains no
database, user credential, CA key or configuration authority. The desktop
cannot register its own expected checksums.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

spec = importlib.util.spec_from_file_location("host_runtime_producer", Path(__file__).with_name("prepare-host-runtime.py"))
producer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(producer)

def assemble(args):
    registration = json.loads(producer.regular(args.inputs, 64 * 1024), object_pairs_hook=producer.unique_object)
    if set(registration) != {"schema_version", "qualified_os", "artifacts"} or registration["schema_version"] != 1:
        raise ValueError("invalid input assembly schema")
    if registration["qualified_os"] not in ("ubuntu-24.04", "fedora-42"):
        raise ValueError("unsupported Host target")
    if set(registration["artifacts"]) != {"server", "postgresql", "caddy"}:
        raise ValueError("complete Home input set required")
    if args.output.exists() or args.output.is_symlink():
        raise ValueError("existing input bundle is preserved")
    parent = args.output.absolute().parent
    if any(part.is_symlink() for part in (parent, *parent.parents)):
        raise ValueError("redirected input bundle parent")
    parent.mkdir(parents=True, exist_ok=True)
    temporary = Path(tempfile.mkdtemp(prefix=".host-inputs-", dir=parent))
    try:
        (temporary / "cache").mkdir(mode=0o755)
        (temporary / "attestations").mkdir(mode=0o755)
        identities = {}
        for kind, entry in registration["artifacts"].items():
            if set(entry) != {"manifest", "signature", "trusted_keyring", "input"}:
                raise ValueError("closed supplied input fields required")
            paths = {key: Path(value) for key, value in entry.items()}
            if any(not path.is_absolute() for path in paths.values()):
                raise ValueError("explicit absolute input paths required")
            manifest = json.loads(producer.regular(paths["manifest"], 2 * 1024 * 1024), object_pairs_hook=producer.unique_object)
            if manifest["kind"] != kind or manifest["qualified_os"] != registration["qualified_os"]:
                raise ValueError("input kind/platform identity conflict")
            producer.publish(argparse.Namespace(output=temporary / "cache", **paths))
            raw_manifest = producer.regular(paths["manifest"], 2 * 1024 * 1024)
            identities[kind] = producer.digest(raw_manifest)
            attestation = temporary / "attestations" / kind
            attestation.mkdir(mode=0o755)
            for source_name, target_name in (("manifest", "manifest.json"), ("signature", "manifest.sig")):
                data = producer.regular(paths[source_name], 2 * 1024 * 1024)
                with (attestation / target_name).open("xb") as stream:
                    stream.write(data)
                    stream.flush()
                    os.fsync(stream.fileno())
        catalog = {"schema_version": 1, "qualified_os": registration["qualified_os"], "artifacts": identities}
        with (temporary / "catalog.json").open("x") as stream:
            json.dump(catalog, stream, sort_keys=True, indent=2)
            stream.write("\n"); stream.flush(); os.fsync(stream.fileno())
        (temporary / "catalog.json").chmod(0o644)
        with (temporary / "registration.json").open("x") as stream:
            json.dump(catalog, stream, sort_keys=True, indent=2)
            stream.write("\n"); stream.flush(); os.fsync(stream.fileno())
        (temporary / "registration.json").chmod(0o644)
        temporary.chmod(0o755)
        fd = os.open(temporary, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(fd)
        finally:
            os.close(fd)
        os.rename(temporary, args.output)
        fd = os.open(parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(fd)
        finally:
            os.close(fd)
    finally:
        if temporary.exists():
            shutil.rmtree(temporary)

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inputs", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    try:
        assemble(parser.parse_args())
    except (ValueError, OSError, KeyError, subprocess.SubprocessError) as error:
        parser.exit(2, "Host input assembly blocked: " + type(error).__name__ + "\n")
