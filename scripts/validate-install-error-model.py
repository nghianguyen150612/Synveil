#!/usr/bin/env python3
"""Fail-closed P006/P010 bridge and ordinary-copy contract validator."""

import ast
import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "tests/install-error-model/p006-acquisition-codes.json"
SOURCE = ROOT / "scripts/release_download.py"
COPY = ROOT / "docs/v0.2/INSTALLER_ERROR_MODEL.md"
FORBIDDEN_COPY = (
    "DATABASE_URL", "SQLite", "PostgreSQL", "systemd", "Task Scheduler",
    "IPC", "SecretStore", "OutcomeUnknown", "EngineErrorCode", "JournalErrorCode",
)


def acquisition_codes(path: pathlib.Path) -> list[str]:
    tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
    codes: list[str] = []
    for node in ast.walk(tree):
        if not isinstance(node, ast.Call):
            continue
        called = node.func
        if not (isinstance(called, ast.Name) and called.id == "AcquisitionError"):
            continue
        if not node.args or not isinstance(node.args[0], ast.Constant) or not isinstance(node.args[0].value, str):
            raise ValueError(f"dynamic AcquisitionError code at line {node.lineno}")
        codes.append(node.args[0].value)
    return codes


def validate() -> None:
    fixture = json.loads(FIXTURE.read_text(encoding="utf-8"))
    if fixture != sorted(fixture) or len(fixture) != len(set(fixture)):
        raise ValueError("acquisition fixture must be sorted and unique")
    if set(fixture) != set(acquisition_codes(SOURCE)):
        raise ValueError("P006 AcquisitionError literals differ from bridge fixture")
    text = COPY.read_text(encoding="utf-8")
    start = text.index("<!-- ORDINARY_COPY_START -->")
    end = text.index("<!-- ORDINARY_COPY_END -->")
    ordinary = text[start:end]
    for term in FORBIDDEN_COPY:
        if term.lower() in ordinary.lower():
            raise ValueError(f"ordinary reference copy contains forbidden term: {term}")
    rows = [line for line in ordinary.splitlines() if line.startswith("| `installer.error.")]
    if len(rows) != 19 or any(row.count("|") != 4 for row in rows):
        raise ValueError("ordinary copy must contain 19 complete English/Vietnamese rows")


if __name__ == "__main__":
    try:
        validate()
    except (ValueError, OSError, SyntaxError, json.JSONDecodeError) as error:
        print(f"install error model validation failed: {error}", file=sys.stderr)
        raise SystemExit(1)
    print("install error model bridge and copy validation passed")
