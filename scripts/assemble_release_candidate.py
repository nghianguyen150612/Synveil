#!/usr/bin/env python3
"""Assemble independently produced Synveil artifacts into one verified manifest."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import stat
import sys
import tomllib

import release_manifest

MAX_MANIFEST_BYTES = 1024 * 1024
REQUIRED_TYPES = {"deb", "rpm", "appimage", "windows_installer", "windows_portable_zip"}


class AssemblyError(ValueError):
    pass


def _unique_object(pairs: list[tuple[str, object]]) -> dict:
    result = {}
    for key, value in pairs:
        if key in result:
            raise AssemblyError("producer manifest contains a duplicate JSON field")
        result[key] = value
    return result


def _read_regular(path: Path, limit: int | None = None) -> bytes:
    try:
        before = path.lstat()
        if not stat.S_ISREG(before.st_mode) or before.st_nlink != 1:
            raise AssemblyError("candidate input is not a single-link regular file")
        if limit is not None and before.st_size > limit:
            raise AssemblyError("producer manifest exceeds its size limit")
        fd = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_CLOEXEC", 0))
    except OSError as error:
        raise AssemblyError("candidate input is missing or redirected") from error
    try:
        opened = os.fstat(fd)
        if (not stat.S_ISREG(opened.st_mode) or opened.st_nlink != 1
                or (opened.st_dev, opened.st_ino) != (before.st_dev, before.st_ino)):
            raise AssemblyError("candidate input changed during identity check")
        with os.fdopen(fd, "rb", closefd=False) as stream:
            data = stream.read((limit + 1) if limit is not None else before.st_size + 1)
    finally:
        os.close(fd)
    if len(data) != before.st_size or (limit is not None and len(data) > limit):
        raise AssemblyError("candidate input changed or exceeded its size limit")
    return data


def _version() -> str:
    cargo = Path(__file__).resolve().parents[1] / "Cargo.toml"
    try:
        return tomllib.loads(cargo.read_text(encoding="utf-8"))["workspace"]["package"]["version"]
    except (OSError, UnicodeError, tomllib.TOMLDecodeError, KeyError, TypeError) as error:
        raise AssemblyError("cannot determine the authoritative Cargo workspace version") from error


def _manifest(path: Path, source_commit: str, version: str) -> dict:
    raw = _read_regular(path, MAX_MANIFEST_BYTES)
    try:
        document = json.loads(raw.decode("utf-8"), object_pairs_hook=_unique_object)
    except (UnicodeError, json.JSONDecodeError) as error:
        raise AssemblyError("producer manifest is not valid UTF-8 JSON") from error
    try:
        document = release_manifest.validate(document, path.parent, source_commit)
    except release_manifest.ManifestError as error:
        raise AssemblyError(f"producer manifest failed validation: {path.name}: {error}") from error
    if document["product_version"] != version:
        raise AssemblyError("producer manifest version differs from Cargo workspace version")
    if not document["artifacts"]:
        raise AssemblyError("producer manifest contains no artifacts")
    return document


def assemble(input_root: Path, output_root: Path, source_commit: str) -> dict:
    if len(source_commit) != 40 or any(character not in "0123456789abcdef" for character in source_commit):
        raise AssemblyError("source commit must be a full lowercase Git SHA")
    input_root = input_root.absolute()
    try:
        info = input_root.lstat()
    except OSError as error:
        raise AssemblyError("candidate input root is missing") from error
    if not stat.S_ISDIR(info.st_mode) or input_root.is_symlink():
        raise AssemblyError("candidate input root is not a real directory")
    if output_root.exists() or output_root.is_symlink():
        raise AssemblyError("candidate output root already exists; existing files are preserved")
    for parent in output_root.absolute().parents:
        if parent.is_symlink():
            raise AssemblyError("candidate output ancestry contains a symlink")

    manifests: list[Path] = []
    for directory, dirnames, filenames in os.walk(input_root, followlinks=False):
        current = Path(directory)
        if current.is_symlink():
            raise AssemblyError("candidate input tree contains a symlink directory")
        for name in dirnames:
            if (current / name).is_symlink():
                raise AssemblyError("candidate input tree contains a symlink directory")
        if "SYNVEIL-RELEASE-MANIFEST.json" in filenames:
            manifests.append(current / "SYNVEIL-RELEASE-MANIFEST.json")
    if len(manifests) not in {3, 4}:
        raise AssemblyError("candidate must contain Linux and AppImage manifests plus one or two Windows manifests")

    version = _version()
    documents = [_manifest(path, source_commit, version) for path in sorted(manifests)]
    artifacts = [item for document in documents for item in document["artifacts"]]
    if len(artifacts) != 5 or {item["artifact_type"] for item in artifacts} != REQUIRED_TYPES:
        raise AssemblyError("candidate artifact set must contain exactly DEB, RPM, AppImage, Windows Setup and portable ZIP")
    filenames = [item["filename"] for item in artifacts]
    if len(filenames) != len(set(filenames)):
        raise AssemblyError("candidate artifacts contain duplicate filenames")
    identifiers = [item["id"] for item in artifacts]
    if len(identifiers) != len(set(identifiers)):
        raise AssemblyError("candidate artifacts contain duplicate identities")

    output_root.mkdir(mode=0o700, parents=True)
    try:
        for manifest_path, document in zip(sorted(manifests), documents, strict=True):
            for artifact in document["artifacts"]:
                source = manifest_path.parent / artifact["filename"]
                destination = output_root / artifact["filename"]
                if destination.exists() or destination.is_symlink():
                    raise AssemblyError("candidate artifact destination collision")
                source_bytes = _read_regular(source)
                if len(source_bytes) != artifact["size_bytes"] or hashlib.sha256(source_bytes).hexdigest() != artifact["sha256"]:
                    raise AssemblyError("producer artifact changed after manifest validation")
                flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0)
                fd = os.open(destination, flags, 0o600)
                with os.fdopen(fd, "wb") as stream:
                    stream.write(source_bytes)
                    stream.flush()
                    os.fsync(stream.fileno())
                if artifact["artifact_type"] == "appimage":
                    destination.chmod(0o755)
        combined = {
            "schema_version": 1,
            "product": "Synveil",
            "product_version": version,
            "source_commit": source_commit,
            "artifacts": sorted(artifacts, key=lambda item: item["id"]),
        }
        release_manifest.validate(combined, output_root, source_commit)
        release_manifest.write(output_root / "SYNVEIL-RELEASE-MANIFEST.json", combined)
        release_manifest.validate(combined, output_root, source_commit)
        summary = {
            "schema_version": 1,
            "status": "VERIFIED_UNSIGNED_CANDIDATE",
            "product_version": version,
            "source_commit": source_commit,
            "artifacts": [
                {"id": item["id"], "artifact_type": item["artifact_type"],
                 "filename": item["filename"], "size_bytes": item["size_bytes"],
                 "sha256": item["sha256"]}
                for item in combined["artifacts"]
            ],
            "production_readiness": "blocked pending approved signing, P036 inputs, and native qualification",
        }
        (output_root / "candidate-evidence.json").write_text(
            json.dumps(summary, sort_keys=True, indent=2) + "\n", encoding="utf-8", newline="\n"
        )
        return summary
    except Exception:
        shutil.rmtree(output_root, ignore_errors=True)
        raise


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input-root", type=Path, required=True)
    parser.add_argument("--output-root", type=Path, required=True)
    parser.add_argument("--source-commit", required=True)
    args = parser.parse_args()
    try:
        print(json.dumps(assemble(args.input_root, args.output_root, args.source_commit), sort_keys=True))
        return 0
    except (AssemblyError, release_manifest.ManifestError, OSError, ValueError, TypeError) as error:
        print(f"release-candidate-assembly: ERROR: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
