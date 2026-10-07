#!/usr/bin/env python3
"""Network-free model of the bounded P027 Windows lifecycle decisions."""

from __future__ import annotations

from dataclasses import dataclass
import re
from pathlib import PureWindowsPath

APP_ID = "{7DDE2E8A-376A-4FC8-96FF-7DB529F0945D}"
PRESERVED_CLASSES = frozenset({
    "APPLICATION_CONFIG", "CREDENTIAL_STATE", "CLIENT_SYNC_STATE", "USER_LIBRARY",
    "SERVER_CONFIG", "SERVER_DATABASE", "SERVER_OBJECT_DATA",
})
PURGE_CLASSES = frozenset({"APPLICATION_CONFIG"})


@dataclass(frozen=True, order=True)
class StableVersion:
    major: int
    minor: int
    patch: int

    @classmethod
    def parse(cls, value: str) -> "StableVersion":
        parts = value.split(".")
        if len(value) > 32 or len(parts) != 3 or any(not re.fullmatch(r"0|[1-9][0-9]{0,9}", p) or int(p) > 2**32 - 1 for p in parts):
            raise ValueError("unknown version")
        return cls(*(int(part) for part in parts))


def lifecycle(installed: str | None, target: str, repair: bool, silent: bool = True, compatible_sources: frozenset[str] = frozenset()) -> str:
    target_version = StableVersion.parse(target)
    if installed is None:
        if repair:
            raise ValueError("repair requires installed identity")
        return "install"
    installed_version = StableVersion.parse(installed)
    if installed_version > target_version:
        raise ValueError("downgrade rejected")
    if installed_version == target_version:
        if silent and not repair:
            raise ValueError("silent repair must be explicit")
        return "repair"
    if repair:
        raise ValueError("repair version mismatch")
    if installed not in compatible_sources:
        raise ValueError("unknown upgrade compatibility")
    return "upgrade"


def obsolete_owned(previous: set[str], target: set[str], reparse: set[str] = frozenset()) -> set[str]:
    result: set[str] = set()
    for value in previous - target:
        normalized = value.replace("/", "\\")
        path = PureWindowsPath(normalized)
        if (path.is_absolute() or path.drive or not value or len(value) > 240
                or any(ord(c) < 32 or 127 <= ord(c) <= 159 or c in ":<>\"|?*" for c in value)
                or any(not part or part in {".", ".."} or part != part.strip() or part.endswith(".")
                       or re.fullmatch(r"(?i:CON|PRN|AUX|NUL|CONIN\$|CONOUT\$|COM[1-9]|LPT[1-9])", part.split(".")[0])
                       for part in normalized.split("\\"))
                or any(str(parent).casefold() in {v.replace("/", "\\").casefold() for v in reparse}
                       for parent in (path, *path.parents))):
            raise ValueError("unsafe obsolete identity")
        result.add(str(path))
    return result


def authorize_purge(classes: set[str], confirmed: bool) -> frozenset[str]:
    if not confirmed or not classes or not classes <= PURGE_CLASSES:
        raise ValueError("destructive authorization rejected")
    return frozenset(classes)
