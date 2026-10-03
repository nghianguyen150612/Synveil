#!/usr/bin/env python3
"""Network-free model of the bounded P027 Windows lifecycle decisions."""

from __future__ import annotations

from dataclasses import dataclass
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
        if len(parts) != 3 or any(not p.isdigit() or (len(p) > 1 and p[0] == "0") for p in parts):
            raise ValueError("unknown version")
        return cls(*(int(part) for part in parts))


def lifecycle(installed: str | None, target: str, repair: bool, silent: bool = True) -> str:
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
    return "upgrade"


def obsolete_owned(previous: set[str], target: set[str], reparse: set[str] = frozenset()) -> set[str]:
    result: set[str] = set()
    for value in previous - target:
        normalized = value.replace("/", "\\")
        path = PureWindowsPath(normalized)
        if path.is_absolute() or ".." in path.parts or path.drive or value in reparse:
            raise ValueError("unsafe obsolete identity")
        result.add(str(path))
    return result


def authorize_purge(classes: set[str], confirmed: bool) -> frozenset[str]:
    if not confirmed or not classes or not classes <= PURGE_CLASSES:
        raise ValueError("destructive authorization rejected")
    return frozenset(classes)
