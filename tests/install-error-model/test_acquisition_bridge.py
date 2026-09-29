#!/usr/bin/env python3
from __future__ import annotations

import ast
import json
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
P006 = ROOT / "scripts" / "release_download.py"
FIXTURE = ROOT / "tests" / "install-error-model" / "p006-acquisition-codes.json"
RUST = ROOT / "crates" / "install-engine" / "src" / "error_model.rs"


def fixture() -> list[str]:
    return json.loads(FIXTURE.read_text(encoding="utf-8"))


def acquisition_calls() -> tuple[list[str], list[int]]:
    tree = ast.parse(P006.read_text(encoding="utf-8"), filename=str(P006))
    codes: list[str] = []
    dynamic: list[int] = []
    for node in ast.walk(tree):
        if not isinstance(node, ast.Call):
            continue
        if not isinstance(node.func, ast.Name) or node.func.id != "AcquisitionError":
            continue
        if node.args and isinstance(node.args[0], ast.Constant) and isinstance(node.args[0].value, str):
            codes.append(node.args[0].value)
        else:
            dynamic.append(node.lineno)
    return sorted(set(codes)), dynamic


class AcquisitionBridgeTests(unittest.TestCase):
    def test_fixture_is_sorted_unique_and_exact_count(self) -> None:
        values = fixture()
        self.assertEqual(values, sorted(values))
        self.assertEqual(len(values), len(set(values)))
        self.assertEqual(len(values), 19)

    def test_p006_source_matches_fixture(self) -> None:
        codes, _ = acquisition_calls()
        self.assertEqual(codes, fixture())

    def test_p006_uses_only_literal_error_codes(self) -> None:
        _, dynamic = acquisition_calls()
        self.assertEqual(dynamic, [])

    def test_rust_contract_contains_every_fixture_code(self) -> None:
        text = RUST.read_text(encoding="utf-8")
        for code in fixture():
            self.assertIn(f'"{code}"', text)


if __name__ == "__main__":
    unittest.main(verbosity=2)
