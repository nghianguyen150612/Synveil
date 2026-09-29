#!/usr/bin/env python3
"""Validate Prompt010's cross-language P006 acquisition error bridge."""

from __future__ import annotations

import ast
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
P006 = ROOT / "scripts" / "release_download.py"
FIXTURE = ROOT / "tests" / "install-error-model" / "p006-acquisition-codes.json"
RUST = ROOT / "crates" / "install-engine" / "src" / "error_model.rs"


def load_fixture() -> list[str]:
    values = json.loads(FIXTURE.read_text(encoding="utf-8"))
    if not isinstance(values, list) or not all(isinstance(value, str) for value in values):
        raise SystemExit("P006 acquisition fixture must be a JSON string array")
    if values != sorted(values):
        raise SystemExit("P006 acquisition fixture must be sorted")
    if len(values) != len(set(values)):
        raise SystemExit("P006 acquisition fixture contains duplicates")
    if len(values) != 19:
        raise SystemExit(f"expected 19 P006 acquisition codes, found {len(values)}")
    return values


def source_codes() -> list[str]:
    tree = ast.parse(P006.read_text(encoding="utf-8"), filename=str(P006))
    codes: list[str] = []
    dynamic: list[int] = []
    for node in ast.walk(tree):
        if not isinstance(node, ast.Call):
            continue
        if not isinstance(node.func, ast.Name) or node.func.id != "AcquisitionError":
            continue
        if not node.args:
            dynamic.append(node.lineno)
            continue
        first = node.args[0]
        if isinstance(first, ast.Constant) and isinstance(first.value, str):
            codes.append(first.value)
        else:
            dynamic.append(node.lineno)
    if dynamic:
        raise SystemExit(f"dynamic AcquisitionError code at lines: {dynamic}")
    return sorted(set(codes))


def rust_codes() -> list[str]:
    text = RUST.read_text(encoding="utf-8")
    fixture = load_fixture()
    missing = [code for code in fixture if f'"{code}"' not in text]
    if missing:
        raise SystemExit(f"Rust acquisition mapping missing codes: {missing}")
    return fixture


def main() -> None:
    fixture = load_fixture()
    source = source_codes()
    if source != fixture:
        raise SystemExit(
            "P006 acquisition code drift: "
            f"source={source!r} fixture={fixture!r}"
        )
    rust_codes()
    print("installer error model acquisition bridge valid: 19 exact P006 codes")


if __name__ == "__main__":
    main()
