#!/usr/bin/env python3
"""Audit the v0.2 candidate version across Cargo and native package policy."""

from __future__ import annotations

import argparse
import glob
import json
from pathlib import Path
import re
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]
STABLE_VERSION = re.compile(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\Z")


class VersionAuditError(ValueError):
    pass


def audit(expected: str, source_commit: str | None = None) -> dict:
    try:
        cargo = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
        lock = tomllib.loads((ROOT / "Cargo.lock").read_text(encoding="utf-8"))
    except (OSError, UnicodeError, tomllib.TOMLDecodeError) as error:
        raise VersionAuditError("Cargo version metadata cannot be read") from error
    actual = cargo.get("workspace", {}).get("package", {}).get("version")
    if actual != expected or not isinstance(actual, str) or not STABLE_VERSION.fullmatch(actual):
        raise VersionAuditError("Cargo workspace version differs from the reviewed release candidate")

    members = cargo.get("workspace", {}).get("members")
    if not isinstance(members, list) or not members:
        raise VersionAuditError("Cargo workspace member inventory is unavailable")
    package_names = set()
    for pattern in members:
        if not isinstance(pattern, str):
            raise VersionAuditError("Cargo workspace member path is malformed")
        matches = sorted(glob.glob(str(ROOT / pattern)))
        if not matches:
            raise VersionAuditError("Cargo workspace member glob has no matches")
        for member in matches:
            manifest_path = Path(member) / "Cargo.toml"
            try:
                package = tomllib.loads(manifest_path.read_text(encoding="utf-8"))["package"]
            except (OSError, UnicodeError, tomllib.TOMLDecodeError, KeyError, TypeError) as error:
                raise VersionAuditError("Cargo workspace member manifest is invalid") from error
            name = package.get("name")
            package_version = package.get("version", {})
            if package_version == {"workspace": True}:
                package_version = actual
            if not isinstance(name, str) or package_version != actual:
                raise VersionAuditError("workspace package does not inherit the authoritative product version")
            package_names.add(name)

    lock_packages = lock.get("package", [])
    for name in sorted(package_names):
        candidates = [item for item in lock_packages if item.get("name") == name and "source" not in item]
        if len(candidates) != 1 or candidates[0].get("version") != actual:
            raise VersionAuditError(f"Cargo.lock version is inconsistent for workspace crate {name}")

    linux_contract = json.loads((ROOT / "deploy/linux/package-integration-v1.json").read_text(encoding="utf-8"))
    if linux_contract.get("product_version_source") != "Cargo.toml:[workspace.package].version":
        raise VersionAuditError("DEB/RPM version source is not the Cargo workspace")
    linux_engine = (ROOT / "crates/install-engine/src/linux_package.rs").read_text(encoding="utf-8")
    if 'const PRODUCT_VERSION: &str = env!("CARGO_PKG_VERSION");' not in linux_engine:
        raise VersionAuditError("Linux package contract must derive its version from Cargo metadata")
    windows_producer = (ROOT / "scripts/build-windows-installer.ps1").read_text(encoding="utf-8")
    if "function Get-WorkspaceVersion" not in windows_producer:
        raise VersionAuditError("Windows installer version is not derived from Cargo workspace metadata")
    appimage_producer = (ROOT / "deploy/packages/build-appimage.sh").read_text(encoding="utf-8")
    if "synveil_cargo_version" not in appimage_producer:
        raise VersionAuditError("AppImage version is not derived from the Cargo workspace")
    for policy_name in ("native-upgrade-policy.json", "windows-upgrade-policy.json"):
        policy = json.loads((ROOT / "deploy/release" / policy_name).read_text(encoding="utf-8"))
        if set(policy) != {"schema_version", "upgrade_from"} or policy.get("schema_version") != 1:
            raise VersionAuditError(f"{policy_name} has an unexpected version policy schema")
        if policy.get("upgrade_from") != []:
            raise VersionAuditError(f"{policy_name} authorizes an unqualified product upgrade")

    if source_commit is not None:
        try:
            head = subprocess.run(["git", "-C", str(ROOT), "rev-parse", "--verify", "HEAD^{commit}"],
                                  check=True, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                  stderr=subprocess.DEVNULL, timeout=10).stdout.decode("ascii").strip()
        except (OSError, UnicodeError, subprocess.SubprocessError) as error:
            raise VersionAuditError("source checkout does not identify a real Git commit") from error
        if not re.fullmatch(r"[0-9a-f]{40}", head) or head != source_commit:
            raise VersionAuditError("source revision differs from the reviewed GitHub commit")
    return {"product_version": actual, "workspace_crates": len(package_names), "source_commit": source_commit}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--expected-version", default="0.2.0")
    parser.add_argument("--source-commit")
    args = parser.parse_args()
    try:
        print(json.dumps(audit(args.expected_version, args.source_commit), sort_keys=True))
        return 0
    except (VersionAuditError, OSError, UnicodeError, json.JSONDecodeError, ValueError, TypeError) as error:
        print(f"release-version-audit: ERROR: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
