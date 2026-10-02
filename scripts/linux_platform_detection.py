#!/usr/bin/env python3
"""Safely detect and qualify the current Linux execution environment."""

from __future__ import annotations

import argparse
import json
import platform
import re
import shlex
import shutil
import sys
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Callable

MAX_OS_RELEASE_BYTES = 64 * 1024
ROOT = Path(__file__).resolve().parents[1]
DEFAULT_POLICY = ROOT / "deploy/install/linux-platforms-v1.json"
CRITICAL_FIELDS = frozenset({"ID", "VERSION_ID", "ID_LIKE"})
KNOWN_UNQUALIFIED_IDS = frozenset({"debian", "linuxmint", "rocky", "arch", "cachyos"})
ARCHITECTURES = {"x86_64": "x86_64", "amd64": "x86_64", "aarch64": "aarch64", "arm64": "aarch64"}
VALID_PROFILES = frozenset({"debian-x86_64", "fedora-x86_64"})
VALID_ARTIFACTS = frozenset({"deb", "rpm"})
VALID_MANAGERS = frozenset({"APT", "DNF"})
EXACT_VERSION = re.compile(r"^[0-9]+(?:\.[0-9]+)*$")
KEY = re.compile(r"^[A-Z][A-Z0-9_]*$")


class DetectionError(ValueError):
    pass


@dataclass(frozen=True)
class DetectionResult:
    schema_version: int = 1
    os_id: str | None = None
    version_id: str | None = None
    architecture: str | None = None
    qualification_status: str = "MALFORMED_HOST_METADATA"
    target_id: str | None = None
    profile: str | None = None
    artifact_type: str | None = None
    package_manager: str | None = None
    reason_code: str = "uninitialized"

    def to_json(self) -> str:
        return json.dumps(asdict(self), sort_keys=True, separators=(",", ":"))


def _read_bounded(path: Path) -> str:
    try:
        size = path.stat().st_size
        if size > MAX_OS_RELEASE_BYTES:
            raise DetectionError("os-release exceeds the size limit")
        raw = path.read_bytes()
    except OSError as error:
        raise DetectionError("os-release could not be read") from error
    if len(raw) > MAX_OS_RELEASE_BYTES or b"\0" in raw:
        raise DetectionError("os-release is invalid or too large")
    try:
        return raw.decode("utf-8")
    except UnicodeDecodeError as error:
        raise DetectionError("os-release is not UTF-8") from error


def parse_os_release(path: Path) -> dict[str, str]:
    """Parse os-release as bounded data; no shell content is ever executed."""
    values: dict[str, str] = {}
    for number, raw_line in enumerate(_read_bounded(path).splitlines(), 1):
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue
        key, separator, raw_value = line.partition("=")
        if not separator or not KEY.fullmatch(key):
            raise DetectionError(f"malformed os-release line {number}")
        if key in CRITICAL_FIELDS and key in values:
            raise DetectionError(f"duplicate critical field {key}")
        try:
            lexer = shlex.shlex(raw_value, posix=True)
            lexer.whitespace_split = True
            lexer.commenters = "#"
            tokens = list(lexer)
        except ValueError as error:
            raise DetectionError(f"malformed quoted value for {key}") from error
        if len(tokens) > 1:
            raise DetectionError(f"unquoted whitespace in {key}")
        values[key] = tokens[0] if tokens else ""
    for required in ("ID", "VERSION_ID"):
        if not values.get(required):
            raise DetectionError(f"missing {required}")
    return values


def select_os_release(etc_path: Path = Path("/etc/os-release"),
                      usr_path: Path = Path("/usr/lib/os-release")) -> Path:
    if etc_path.is_file():
        return etc_path
    if usr_path.is_file():
        return usr_path
    raise DetectionError("no os-release metadata found")


def load_policy(path: Path = DEFAULT_POLICY) -> dict:
    try:
        policy = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise DetectionError("qualification policy is unreadable") from error
    validate_policy(policy)
    return policy


def validate_policy(policy: object) -> None:
    if not isinstance(policy, dict) or set(policy) != {"schema_version", "targets"}:
        raise DetectionError("policy has unexpected top-level fields")
    if policy["schema_version"] != 1 or not isinstance(policy["targets"], list):
        raise DetectionError("policy schema version or targets is invalid")
    ids: set[str] = set()
    tuples: set[tuple[str, str, str]] = set()
    required = {"id", "os_id", "version_id", "architecture", "profile", "artifact_type",
                "package_manager", "required_commands"}
    for target in policy["targets"]:
        if not isinstance(target, dict) or set(target) != required:
            raise DetectionError("policy target fields are invalid")
        if not all(isinstance(target[key], str) and target[key] for key in required - {"required_commands"}):
            raise DetectionError("policy target strings must be non-empty")
        if target["id"] in ids:
            raise DetectionError("policy target IDs must be unique")
        identity = (target["os_id"], target["version_id"], target["architecture"])
        if identity in tuples:
            raise DetectionError("policy identity tuples must be unique")
        if not EXACT_VERSION.fullmatch(target["version_id"]):
            raise DetectionError("policy versions must be exact numeric versions")
        if target["architecture"] != "x86_64" or target["profile"] not in VALID_PROFILES:
            raise DetectionError("policy architecture or profile is unknown")
        if target["artifact_type"] not in VALID_ARTIFACTS or target["package_manager"] not in VALID_MANAGERS:
            raise DetectionError("policy artifact or manager is unknown")
        commands = target["required_commands"]
        if not isinstance(commands, list) or not commands or not all(isinstance(item, str) and item for item in commands):
            raise DetectionError("required commands are invalid")
        expected = {"deb": ("debian-x86_64", "APT", ["apt-get", "dpkg-query"]),
                    "rpm": ("fedora-x86_64", "DNF", ["dnf", "rpm"])}[target["artifact_type"]]
        if (target["profile"], target["package_manager"], commands) != expected:
            raise DetectionError("policy package mapping is inconsistent")
        ids.add(target["id"])
        tuples.add(identity)


def detect(*, etc_path: Path = Path("/etc/os-release"), usr_path: Path = Path("/usr/lib/os-release"),
           machine: str | None = None, policy_path: Path = DEFAULT_POLICY,
           command_lookup: Callable[[str], str | None] = shutil.which) -> DetectionResult:
    architecture_raw = (machine or platform.machine()).lower()
    architecture = ARCHITECTURES.get(architecture_raw, architecture_raw or None)
    try:
        values = parse_os_release(select_os_release(etc_path, usr_path))
        policy = load_policy(policy_path)
    except DetectionError as error:
        return DetectionResult(architecture=architecture, reason_code=str(error))
    os_id, version_id = values["ID"].lower(), values["VERSION_ID"]
    base = {"os_id": os_id, "version_id": version_id, "architecture": architecture}
    if architecture != "x86_64":
        reason = "architecture_not_qualified" if architecture in {"aarch64", "i386", "i486", "i586", "i686", "x86", "armv7l", "armv7"} else "unknown_architecture"
        return DetectionResult(**base, qualification_status="UNSUPPORTED_ARCHITECTURE", reason_code=reason)
    target = next((item for item in policy["targets"]
                   if (item["os_id"], item["version_id"], item["architecture"]) ==
                   (os_id, version_id, architecture)), None)
    if target is None:
        policy_ids = {item["os_id"] for item in policy["targets"]}
        if os_id in policy_ids:
            status, reason = "UNKNOWN_VERSION", "version_not_qualified"
        elif os_id in KNOWN_UNQUALIFIED_IDS:
            status, reason = "DETECTED_UNSUPPORTED", "distribution_not_qualified"
        else:
            status, reason = "UNKNOWN_DISTRIBUTION", "distribution_not_in_policy"
        return DetectionResult(**base, qualification_status=status, reason_code=reason)
    common = dict(base, target_id=target["id"], profile=target["profile"],
                  artifact_type=target["artifact_type"], package_manager=target["package_manager"])
    missing = [command for command in target["required_commands"] if command_lookup(command) is None]
    if missing:
        return DetectionResult(**common, qualification_status="MISSING_PACKAGE_MANAGER",
                               reason_code="missing_required_commands:" + ",".join(missing))
    return DetectionResult(**common, qualification_status="QUALIFIED", reason_code="qualified_exact_policy_match")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--policy", type=Path, default=DEFAULT_POLICY)
    parser.add_argument("--os-release", type=Path)
    parser.add_argument("--usr-os-release", type=Path, default=Path("/usr/lib/os-release"))
    parser.add_argument("--architecture")
    parser.add_argument("--validate-policy", action="store_true")
    args = parser.parse_args(argv)
    if args.validate_policy:
        try:
            load_policy(args.policy)
        except DetectionError as error:
            print(f"invalid Linux platform policy: {error}", file=sys.stderr)
            return 1
        print("Linux platform policy is valid")
        return 0
    result = detect(etc_path=args.os_release or Path("/etc/os-release"), usr_path=args.usr_os_release,
                    machine=args.architecture, policy_path=args.policy)
    print(result.to_json())
    return 0 if result.qualification_status == "QUALIFIED" else 10


if __name__ == "__main__":
    sys.exit(main())
