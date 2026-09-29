import ast
import importlib.util
import json
import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("validator", ROOT / "scripts/validate-install-error-model.py")
VALIDATOR = importlib.util.module_from_spec(SPEC)
assert SPEC.loader
SPEC.loader.exec_module(VALIDATOR)


class AcquisitionBridgeTests(unittest.TestCase):
    def fixture(self):
        return json.loads(VALIDATOR.FIXTURE.read_text(encoding="utf-8"))

    def test_fixture_is_sorted(self):
        self.assertEqual(self.fixture(), sorted(self.fixture()))

    def test_fixture_has_no_duplicates(self):
        self.assertEqual(len(self.fixture()), len(set(self.fixture())))

    def test_fixture_exactly_matches_python_literals(self):
        self.assertEqual(set(self.fixture()), set(VALIDATOR.acquisition_codes(VALIDATOR.SOURCE)))

    def test_dynamic_code_is_rejected(self):
        tree = ast.parse("AcquisitionError(code, 'detail')")
        call = next(node for node in ast.walk(tree) if isinstance(node, ast.Call))
        self.assertFalse(isinstance(call.args[0], ast.Constant))


if __name__ == "__main__":
    unittest.main()
