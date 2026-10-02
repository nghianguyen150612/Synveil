#!/usr/bin/env python3
import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("linux_platform_detection", ROOT / "scripts/linux_platform_detection.py")
detection = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = detection
assert SPEC.loader
SPEC.loader.exec_module(detection)


class PlatformDetectionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.etc = self.root / "etc-os-release"
        self.usr = self.root / "usr-os-release"
        self.absent = self.root / "absent"

    def tearDown(self):
        self.temp.cleanup()

    def run_detection(self, text=None, machine="x86_64", *, fallback=False, commands=True):
        if text is not None:
            (self.usr if fallback else self.etc).write_text(text, encoding="utf-8")
        return detection.detect(etc_path=self.etc, usr_path=self.usr, machine=machine,
                                command_lookup=(lambda _: "/bin/tool" if commands else None))

    def assert_status(self, expected, text, machine="x86_64"):
        result = self.run_detection(text, machine)
        self.assertEqual(expected, result.qualification_status, result.to_json())
        return result

    def test_qualified_exact_targets_and_aliases(self):
        ubuntu = self.assert_status("QUALIFIED", 'ID=ubuntu\nVERSION_ID="24.04"\n', "amd64")
        self.assertEqual(("x86_64", "debian-x86_64", "deb", "APT"),
                         (ubuntu.architecture, ubuntu.profile, ubuntu.artifact_type, ubuntu.package_manager))
        fedora = self.assert_status("QUALIFIED", "ID=fedora\nVERSION_ID=42\n")
        self.assertEqual(("fedora-42-x86_64", "fedora-x86_64", "rpm", "DNF"),
                         (fedora.target_id, fedora.profile, fedora.artifact_type, fedora.package_manager))

    def test_exact_versions_only(self):
        self.assert_status("UNKNOWN_VERSION", "ID=ubuntu\nVERSION_ID=24.04.1\n")
        self.assert_status("UNKNOWN_VERSION", "ID=ubuntu\nVERSION_ID=26.04\n")
        self.assert_status("UNKNOWN_VERSION", "ID=fedora\nVERSION_ID=43\n")

    def test_debian_is_detected_not_qualified(self):
        result = self.assert_status("DETECTED_UNSUPPORTED", "ID=debian\nVERSION_ID=12\n")
        self.assertEqual("debian", result.os_id)

    def test_id_like_never_inherits_qualification(self):
        mint = self.assert_status("DETECTED_UNSUPPORTED",
                                  'ID=linuxmint\nVERSION_ID=22\nID_LIKE="ubuntu debian"\n')
        rocky = self.assert_status("DETECTED_UNSUPPORTED",
                                   'ID=rocky\nVERSION_ID=9.5\nID_LIKE="rhel centos fedora"\n')
        self.assertIsNone(mint.profile)
        self.assertIsNone(rocky.profile)

    def test_other_distributions_are_not_guessed(self):
        for os_id in ("arch", "cachyos"):
            with self.subTest(os_id=os_id):
                self.assert_status("DETECTED_UNSUPPORTED", f"ID={os_id}\nVERSION_ID=2026.01\n")

    def test_architectures_normalize_without_fallback(self):
        source = "ID=ubuntu\nVERSION_ID=24.04\n"
        self.assertEqual("x86_64", self.assert_status("QUALIFIED", source, "x86_64").architecture)
        for alias in ("aarch64", "arm64"):
            with self.subTest(alias=alias):
                result = self.assert_status("UNSUPPORTED_ARCHITECTURE", source, alias)
                self.assertEqual("aarch64", result.architecture)
        for value in ("i686", "mips64"):
            with self.subTest(value=value):
                self.assert_status("UNSUPPORTED_ARCHITECTURE", source, value)

    def test_fallback_to_usr_os_release(self):
        result = self.run_detection("ID=fedora\nVERSION_ID=42\n", fallback=True)
        self.assertEqual("QUALIFIED", result.qualification_status)

    def test_missing_and_malformed_metadata(self):
        self.assertEqual("MALFORMED_HOST_METADATA", self.run_detection().qualification_status)
        for text in ('VERSION_ID=24.04\n', 'ID=ubuntu\n', 'ID="ubuntu\nVERSION_ID=24.04\n',
                     'ID=ubuntu\nID=ubuntu\nVERSION_ID=24.04\n'):
            with self.subTest(text=text):
                self.etc.unlink(missing_ok=True)
                self.assertEqual("MALFORMED_HOST_METADATA", self.run_detection(text).qualification_status)

    def test_shell_syntax_is_data_not_execution(self):
        marker = self.root / "executed"
        result = self.run_detection(f'ID="$(touch {marker})"\nVERSION_ID=1\n')
        self.assertFalse(marker.exists())
        self.assertEqual("UNKNOWN_DISTRIBUTION", result.qualification_status)

    def test_missing_manager_is_typed_after_exact_qualification(self):
        result = self.run_detection("ID=ubuntu\nVERSION_ID=24.04\n", commands=False)
        self.assertEqual("MISSING_PACKAGE_MANAGER", result.qualification_status)
        self.assertIn("apt-get", result.reason_code)

    def test_policy_validator_rejects_ambiguous_tuple(self):
        policy = json.loads(detection.DEFAULT_POLICY.read_text(encoding="utf-8"))
        policy["targets"].append(dict(policy["targets"][0], id="duplicate"))
        with self.assertRaises(detection.DetectionError):
            detection.validate_policy(policy)


if __name__ == "__main__":
    unittest.main()
