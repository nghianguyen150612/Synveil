#!/usr/bin/env python3
"""Compile reviewed native upgrade identities into package-owned hooks.

An empty policy authorizes only the hook's existing exact-version repair.
This does not assert migration compatibility or authorize release publication.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import shlex


def _unique_object(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result: dict[str, object] = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate policy field")
        result[key] = value
    return result


def _version(value: object) -> tuple[int, int, int]:
    if not isinstance(value, str) or len(value) > 32:
        raise ValueError("invalid stable product version")
    if re.fullmatch(r"(?:0|[1-9][0-9]{0,9})\.(?:0|[1-9][0-9]{0,9})\.(?:0|[1-9][0-9]{0,9})", value) is None:
        raise ValueError("invalid stable product version")
    parts = tuple(int(part) for part in value.split("."))
    if any(part > 2**32 - 1 for part in parts):
        raise ValueError("invalid stable product version")
    return parts


def compile_sources(policy_path: Path, target_version: str, package_format: str) -> str:
    with policy_path.open("rb") as stream:
        content = stream.read(16 * 1024 + 1)
    if len(content) > 16 * 1024:
        raise ValueError("native policy is too large")
    policy = json.loads(content, object_pairs_hook=_unique_object)
    if not isinstance(policy, dict) or set(policy) != {"schema_version", "upgrade_from"}:
        raise ValueError("unknown native policy schema")
    if type(policy["schema_version"]) is not int or policy["schema_version"] != 1:
        raise ValueError("unknown native policy schema")
    target = _version(target_version)
    entries = policy["upgrade_from"]
    if not isinstance(entries, list) or len(entries) > 32:
        raise ValueError("invalid upgrade source inventory")
    sources: list[str] = []
    seen: set[str] = set()
    for entry in entries:
        if not isinstance(entry, dict) or set(entry) != {"product_version", "rpm_release"}:
            raise ValueError("unknown upgrade source schema")
        version = entry["product_version"]
        if _version(version) >= target or version in seen:
            raise ValueError("upgrade sources must be unique, explicit older versions")
        release = entry["rpm_release"]
        if not isinstance(release, str) or re.fullmatch(r"[1-9][0-9]{0,8}", release) is None:
            raise ValueError("invalid RPM source release")
        seen.add(version)
        if package_format == "deb":
            sources.append(version)
        elif package_format == "rpm":
            # Expand the same distro suffix at rpmbuild time. No wildcard can
            # admit a foreign native release or skip the pre-unpack gate.
            sources.append(f"{version}-{release}%{{?dist}}")
        else:
            raise ValueError("unsupported native package format")
    return " ".join(shlex.quote(source) for source in sources) if sources else "''"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--policy", type=Path, required=True)
    parser.add_argument("--target-version", required=True)
    parser.add_argument("--format", choices=("deb", "rpm"), required=True)
    args = parser.parse_args()
    try:
        print(compile_sources(args.policy, args.target_version, args.format))
    except (OSError, ValueError, TypeError) as error:
        parser.exit(1, f"native upgrade policy rejected: {error}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
