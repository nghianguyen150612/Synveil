#!/usr/bin/env python3
"""Create, validate, inspect, and merge Synveil release manifests (stdlib only)."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path, PurePosixPath, PureWindowsPath

SCHEMA_VERSION = 1
PRODUCT = "Synveil"
TYPES = {"deb", "rpm", "appimage", "windows_installer", "windows_portable_zip"}
PLATFORMS = {"linux", "windows"}
ARCHITECTURES = {"x86_64", "aarch64"}
ROLES = {"native_package", "primary_installer", "portable"}
COMPONENTS = {"synveil-desktop", "synveil-client", "scheduled-maintenance"}
TOP_FIELDS = {"schema_version", "product", "product_version", "source_commit", "artifacts"}
ARTIFACT_FIELDS = {"id", "artifact_type", "filename", "platform", "architecture", "role", "product_version", "size_bytes", "sha256", "components", "package_metadata"}
REQUIRED_ARTIFACT_FIELDS = ARTIFACT_FIELDS - {"package_metadata"}


class ManifestError(ValueError):
    pass


def fail(message: str) -> None:
    raise ManifestError(message)


def safe_filename(value: object) -> bool:
    if not isinstance(value, str) or not value or "\0" in value or "\\" in value:
        return False
    path = PurePosixPath(value)
    return not path.is_absolute() and len(path.parts) == 1 and value not in {".", ".."} and ".." not in path.parts and not PureWindowsPath(value).is_absolute()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def require_fields(obj: dict, required: set[str], allowed: set[str], where: str) -> None:
    missing = sorted(required - obj.keys())
    unknown = sorted(obj.keys() - allowed)
    if missing:
        fail(f"{where}: missing required field: {missing[0]}")
    if unknown:
        fail(f"{where}: unknown field: {unknown[0]}")


def validate_artifact(item: object, product_version: str, artifact_root: Path | None) -> None:
    if not isinstance(item, dict):
        fail("artifact: must be an object")
    identity = item.get("id", "<unknown>")
    where = f"artifact {identity}"
    require_fields(item, REQUIRED_ARTIFACT_FIELDS, ARTIFACT_FIELDS, where)
    if not isinstance(identity, str) or not re.fullmatch(r"[a-z0-9]+(?:[a-z0-9_-]*[a-z0-9])?", identity):
        fail(f"{where}: invalid id")
    kind = item["artifact_type"]
    if kind not in TYPES:
        fail(f"{where}: unknown artifact_type: {kind}")
    if item["platform"] not in PLATFORMS:
        fail(f"{where}: invalid platform")
    if item["architecture"] not in ARCHITECTURES:
        fail(f"{where}: invalid architecture")
    if item["role"] not in ROLES:
        fail(f"{where}: invalid role")
    expected_platform = "windows" if kind.startswith("windows_") else "linux"
    if item["platform"] != expected_platform:
        fail(f"{where}: artifact_type/platform mismatch")
    expected_role = {"deb": "native_package", "rpm": "native_package", "windows_portable_zip": "portable"}.get(kind)
    if expected_role and item["role"] != expected_role:
        fail(f"{where}: artifact_type/role mismatch")
    if item["product_version"] != product_version:
        fail(f"{where}: product_version mismatch")
    if not safe_filename(item["filename"]):
        fail(f"{where}: unsafe artifact path: {item['filename']!r}")
    if type(item["size_bytes"]) is not int or item["size_bytes"] < 0:
        fail(f"{where}: invalid size_bytes")
    if not isinstance(item["sha256"], str) or not re.fullmatch(r"[0-9a-f]{64}", item["sha256"]):
        fail(f"{where}: invalid sha256")
    components = item["components"]
    if not isinstance(components, list) or len(set(map(str, components))) != len(components) or any(x not in COMPONENTS for x in components):
        fail(f"{where}: invalid components")
    package = item.get("package_metadata")
    if kind in {"deb", "rpm"}:
        if not isinstance(package, dict) or package.get("format") != kind:
            fail(f"{where}: invalid package_metadata combination")
        required = {"format", "package_name", "package_version", "package_architecture"}
        if kind == "rpm":
            required.add("package_release")
        require_fields(package, required, required, f"{where} package_metadata")
        if any(not isinstance(package[key], str) or not package[key] for key in required):
            fail(f"{where}: empty package metadata")
        if package["package_version"] != product_version:
            fail(f"{where}: native package version/product_version mismatch")
    elif package is not None:
        fail(f"{where}: package_metadata is only valid for deb/rpm")
    if artifact_root is not None:
        root = artifact_root.resolve()
        path = root / item["filename"]
        if path.is_symlink() or not path.is_file() or root not in path.resolve().parents:
            fail(f"{where}: artifact missing, non-regular, or outside artifact root")
        if path.stat().st_size != item["size_bytes"]:
            fail(f"{where}: artifact size mismatch")
        if sha256(path) != item["sha256"]:
            fail(f"{where}: artifact digest mismatch")


def validate(data: object, artifact_root: Path | None = None, source_commit: str | None = None) -> dict:
    if not isinstance(data, dict):
        fail("manifest: top level must be an object")
    require_fields(data, TOP_FIELDS, TOP_FIELDS, "manifest")
    if type(data["schema_version"]) is not int or data["schema_version"] != SCHEMA_VERSION:
        fail(f"manifest: unsupported schema_version: {data['schema_version']!r}")
    if data["product"] != PRODUCT:
        fail("manifest: unsupported product")
    if not isinstance(data["product_version"], str) or not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.-]+)?", data["product_version"]):
        fail("manifest: invalid product_version")
    if not isinstance(data["source_commit"], str) or not re.fullmatch(r"[0-9a-f]{40}", data["source_commit"]):
        fail("manifest: source_commit must be a full lowercase Git SHA")
    if source_commit and data["source_commit"] != source_commit:
        fail("manifest: source_commit mismatch")
    if not isinstance(data["artifacts"], list):
        fail("manifest: artifacts must be an array")
    seen: set[str] = set()
    prior = ""
    for artifact in data["artifacts"]:
        validate_artifact(artifact, data["product_version"], artifact_root)
        identity = artifact["id"]
        if identity in seen:
            fail(f"manifest: duplicate artifact id: {identity}")
        if identity < prior:
            fail("manifest: artifacts are not sorted by id")
        seen.add(identity)
        prior = identity
    return data


def load(path: Path) -> dict:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        fail(f"{path}: cannot read JSON: {error}")


def write(path: Path, data: dict) -> None:
    validate(data)
    path.write_text(json.dumps(data, ensure_ascii=False, indent=2, sort_keys=True) + "\n", encoding="utf-8", newline="\n")


def artifact_from_spec(root: Path, version: str, raw: str) -> dict:
    try:
        item = json.loads(raw)
    except json.JSONDecodeError as error:
        fail(f"artifact specification is not JSON: {error}")
    if not isinstance(item, dict):
        fail("artifact specification must be an object")
    forbidden = {"size_bytes", "sha256", "product_version"} & item.keys()
    if forbidden:
        fail(f"generated identity field must not be supplied: {sorted(forbidden)[0]}")
    filename = item.get("filename")
    if not safe_filename(filename):
        fail(f"unsafe artifact path: {filename!r}")
    path = root.resolve() / filename
    if path.is_symlink() or not path.is_file() or root.resolve() not in path.resolve().parents:
        fail(f"artifact missing, non-regular, or outside artifact root: {filename}")
    item.update(product_version=version, size_bytes=path.stat().st_size, sha256=sha256(path))
    return item


def command_create(args: argparse.Namespace) -> None:
    root = Path(args.artifact_root)
    artifacts = sorted((artifact_from_spec(root, args.product_version, raw) for raw in args.artifact), key=lambda x: x["id"])
    write(Path(args.output), {"schema_version": 1, "product": PRODUCT, "product_version": args.product_version, "source_commit": args.source_commit, "artifacts": artifacts})


def command_validate(args: argparse.Namespace) -> None:
    path = Path(args.manifest)
    validate(load(path), Path(args.artifact_root) if args.artifact_root else None, args.source_commit)
    print(f"{path}: valid release manifest")


def command_inspect(args: argparse.Namespace) -> None:
    data = validate(load(Path(args.manifest)))
    print(json.dumps(data if args.json else [{"id": a["id"], "artifact_type": a["artifact_type"], "filename": a["filename"], "sha256": a["sha256"]} for a in data["artifacts"]], sort_keys=True, indent=2))


def command_merge(args: argparse.Namespace) -> None:
    manifests = [validate(load(Path(name))) for name in args.manifests]
    if not manifests:
        fail("merge requires at least one manifest")
    base = {key: manifests[0][key] for key in TOP_FIELDS - {"artifacts"}}
    artifacts = []
    for manifest in manifests:
        for key, value in base.items():
            if manifest[key] != value:
                fail(f"merge: mismatched {key}")
        artifacts.extend(manifest["artifacts"])
    output = dict(base, artifacts=sorted(artifacts, key=lambda x: x["id"]))
    validate(output)
    write(Path(args.output), output)


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    commands = result.add_subparsers(dest="command", required=True)
    create = commands.add_parser("create")
    create.add_argument("--artifact-root", required=True); create.add_argument("--product-version", required=True)
    create.add_argument("--source-commit", required=True); create.add_argument("--output", required=True)
    create.add_argument("--artifact", action="append", default=[]); create.set_defaults(function=command_create)
    check = commands.add_parser("validate"); check.add_argument("manifest")
    check.add_argument("--artifact-root"); check.add_argument("--source-commit"); check.set_defaults(function=command_validate)
    inspect = commands.add_parser("inspect"); inspect.add_argument("manifest"); inspect.add_argument("--json", action="store_true"); inspect.set_defaults(function=command_inspect)
    merge = commands.add_parser("merge"); merge.add_argument("--output", required=True); merge.add_argument("manifests", nargs="+"); merge.set_defaults(function=command_merge)
    return result


def main() -> int:
    try:
        args = parser().parse_args()
        args.function(args)
        return 0
    except ManifestError as error:
        print(f"release-manifest: ERROR: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
