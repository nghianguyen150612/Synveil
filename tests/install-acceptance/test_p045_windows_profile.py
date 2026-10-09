"""Structural security regressions; never native Windows acceptance evidence."""
import importlib.util
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("windows_installer_validator", ROOT / "scripts/validate-windows-installer.py")
validator = importlib.util.module_from_spec(spec)
spec.loader.exec_module(validator)


class TokenProfileProofTests(unittest.TestCase):
    def setUp(self):
        self.helper = validator.KNOWN_FOLDERS.read_text()
        self.children = tuple(p.read_text() for p in (validator.PER_USER_TEST, validator.LIFECYCLE_TEST))

    def test_both_children_keep_token_derived_environment_proof(self):
        validator.validate_token_profile_environment(self.helper, self.children)

    def test_empty_helper_or_launcher_temp_assignment_is_rejected(self):
        for helper in ("function Set-WindowsTokenProfileEnvironment { }", self.helper.replace("$env:TEMP = $tokenTemp", "$env:TEMP = 'C:\\launcher-temp'"), self.helper.replace("$env:LOCALAPPDATA = $localAppData", "$env:LOCALAPPDATA = $env:TEMP")):
            with self.subTest(helper=helper[:45]), self.assertRaises(AssertionError):
                validator.validate_token_profile_environment(helper, self.children)

    def test_missing_call_or_setup_before_identity_guard_is_rejected(self):
        for index, child in enumerate(self.children):
            for replaced in (child.replace("\nSet-WindowsTokenProfileEnvironment\n", "\n"), "Set-WindowsTokenProfileEnvironment\n" + child.replace("\nSet-WindowsTokenProfileEnvironment\n", "\n")):
                children = list(self.children)
                children[index] = replaced
                with self.subTest(child=index), self.assertRaises(AssertionError):
                    validator.validate_token_profile_environment(self.helper, tuple(children))


if __name__ == "__main__":
    unittest.main()
