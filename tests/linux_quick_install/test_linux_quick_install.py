#!/usr/bin/env python3
import contextlib
import hashlib
import importlib.util
import io
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
SPEC = importlib.util.spec_from_file_location("linux_quick_install", ROOT / "scripts/linux_quick_install.py")
quick = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = quick
assert SPEC.loader
SPEC.loader.exec_module(quick)


class FakeManager:
    installed = None
    install_calls = []
    verified = True

    def __init__(self, profile):
        self.profile = profile

    def installed_version(self):
        return self.installed

    def install(self, path, expected_version):
        self.install_calls.append(path)

    def verify(self, version):
        return self.verified


class QuickInstallTests(unittest.TestCase):
    def setUp(self):
        FakeManager.installed = None
        FakeManager.install_calls = []
        FakeManager.verified = True
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.package = self.root / "synveil.deb"
        self.package.write_bytes(b"verified package")
        self.evidence = {
            "schema_version": 1, "manifest_identity": {"sha256": "a" * 64, "size_bytes": 10},
            "manifest_authentication": {"state": "AUTHENTICATED_PINNED_DIGEST", "method": "pinned_sha256", "key_id": None},
            "artifact_id": "linux-x86_64-deb", "artifact_type": "deb", "role": "native_package",
            "platform": "linux", "architecture": "x86_64", "product_version": "0.2.0",
            "source_commit": "b" * 40, "artifact_size": self.package.stat().st_size,
            "artifact_sha256": hashlib.sha256(self.package.read_bytes()).hexdigest(), "result": "VERIFIED",
            "final_path": str(self.package), "diagnostics_redacted": [],
        }
        self.argv = ["--platform-profile=debian-x86_64", "--channel-url=https://release.example/channel.json",
                     "--trusted-channel-sha256=" + "c" * 64, "--trusted-origin=https://release.example",
                     "--minimum-channel-generation=2", "--yes"]

    def tearDown(self):
        self.temp.cleanup()

    def execute(self, *, evidence=None):
        release = self.evidence if evidence is None else evidence
        output, error = io.StringIO(), io.StringIO()
        detected = quick.linux_platform_detection.DetectionResult(
            os_id="ubuntu", version_id="24.04", architecture="x86_64", qualification_status="QUALIFIED",
            target_id="ubuntu-24.04-x86_64", profile="debian-x86_64", artifact_type="deb",
            package_manager="APT", reason_code="qualified_exact_policy_match")
        with mock.patch.object(quick, "acquire", return_value=release), \
             mock.patch.object(quick.tempfile, "mkdtemp", return_value=str(self.root / "stage")), \
             mock.patch.object(quick.os, "chmod"), mock.patch.object(quick.os, "geteuid", return_value=1000), \
             mock.patch.object(quick.shutil, "rmtree"), contextlib.redirect_stdout(output), contextlib.redirect_stderr(error):
            Path(self.root / "stage").mkdir(exist_ok=True)
            status = quick.run(self.argv, manager_factory=FakeManager, detector=lambda: detected)
        return status, output.getvalue(), error.getvalue()

    def test_exact_verified_package_precedes_apt(self):
        status, output, _ = self.execute()
        self.assertEqual(0, status)
        self.assertEqual([self.package], FakeManager.install_calls)
        self.assertIn("Verified release identity: yes", output)
        self.assertIn("INSTALLED_VERIFIED", output)

    def test_tampered_stage_has_zero_mutation(self):
        evidence = dict(self.evidence, artifact_sha256="0" * 64)
        status, _, error = self.execute(evidence=evidence)
        self.assertEqual(quick.EXIT_INTEGRITY, status)
        self.assertEqual([], FakeManager.install_calls)
        self.assertIn("IntegrityVerificationFailed", error)

    def test_same_version_is_verified_noop(self):
        FakeManager.installed = "0.2.0"
        status, output, _ = self.execute()
        self.assertEqual(0, status)
        self.assertEqual([], FakeManager.install_calls)
        self.assertIn("ALREADY_INSTALLED_VERIFIED", output)

    def test_incomplete_same_version_is_not_reinstalled(self):
        FakeManager.installed = "0.2.0"
        FakeManager.verified = False
        status, _, _ = self.execute()
        self.assertEqual(quick.EXIT_PACKAGE_MANAGER, status)
        self.assertEqual([], FakeManager.install_calls)

    def test_newer_installed_version_rejects_downgrade_before_mutation(self):
        FakeManager.installed = "0.3.0"
        status, _, error = self.execute()
        self.assertEqual(quick.EXIT_PACKAGE_MANAGER, status)
        self.assertEqual([], FakeManager.install_calls)
        self.assertIn("downgrade is rejected", error)

    def test_unexpected_error_after_mutation_requires_reconciliation_without_secret(self):
        with mock.patch.object(FakeManager, "install", side_effect=OSError("secret-token=private")):
            status, _, error = self.execute()
        self.assertEqual(quick.EXIT_UNKNOWN, status)
        self.assertIn("OutcomeUnknown", error)
        self.assertNotIn("private", error)
        self.assertIn("before any retry", error)

    def test_post_install_verification_required(self):
        FakeManager.verified = False
        status, _, error = self.execute()
        self.assertEqual(quick.EXIT_VERIFICATION, status)
        self.assertIn("VerificationFailed", error)

    def test_explicit_profile_cannot_override_detection(self):
        detected = quick.linux_platform_detection.DetectionResult(
            os_id="fedora", version_id="42", architecture="x86_64", qualification_status="QUALIFIED",
            target_id="fedora-42-x86_64", profile="fedora-x86_64", artifact_type="rpm",
            package_manager="DNF", reason_code="qualified_exact_policy_match")
        with self.assertRaises(quick.QuickInstallError) as caught:
            quick.resolve_profile("debian-x86_64", detected)
        self.assertEqual("UnsupportedPlatform", caught.exception.category)

    def test_unsupported_detection_precedes_acquisition(self):
        detected = quick.linux_platform_detection.DetectionResult(
            os_id="linuxmint", version_id="22", architecture="x86_64",
            qualification_status="DETECTED_UNSUPPORTED", reason_code="distribution_not_qualified")
        with mock.patch.object(quick.os, "geteuid", return_value=1000), \
             mock.patch.object(quick, "acquire") as acquire, contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(quick.EXIT_UNSUPPORTED,
                             quick.run(self.argv, manager_factory=FakeManager, detector=lambda: detected))
        acquire.assert_not_called()

    def test_detect_only_has_no_acquisition_or_mutation(self):
        detected = quick.linux_platform_detection.DetectionResult(
            os_id="ubuntu", version_id="24.04", architecture="x86_64", qualification_status="QUALIFIED",
            target_id="ubuntu-24.04-x86_64", profile="debian-x86_64", artifact_type="deb",
            package_manager="APT", reason_code="qualified_exact_policy_match")
        output = io.StringIO()
        with mock.patch.object(quick, "acquire") as acquire, contextlib.redirect_stdout(output):
            self.assertEqual(0, quick.run(["--detect-only"], manager_factory=FakeManager,
                                          detector=lambda: detected))
        acquire.assert_not_called()
        self.assertEqual([], FakeManager.install_calls)
        self.assertEqual("QUALIFIED", json.loads(output.getvalue())["qualification_status"])

    def test_root_whole_script_rejected_before_acquisition(self):
        with mock.patch.object(quick.os, "geteuid", return_value=0), mock.patch.dict(os.environ, {}, clear=True), \
             mock.patch.object(quick, "acquire") as acquire, contextlib.redirect_stderr(io.StringIO()):
            detected = quick.linux_platform_detection.DetectionResult(
                os_id="ubuntu", version_id="24.04", architecture="x86_64", qualification_status="QUALIFIED",
                target_id="ubuntu-24.04-x86_64", profile="debian-x86_64", artifact_type="deb",
                package_manager="APT", reason_code="qualified_exact_policy_match")
            self.assertEqual(quick.EXIT_AUTHORIZATION,
                             quick.run(self.argv, manager_factory=FakeManager, detector=lambda: detected))
        acquire.assert_not_called()

    def test_plan_does_not_handle_passwords(self):
        source = (ROOT / "scripts/linux_quick_install.py").read_text(encoding="utf-8")
        self.assertNotIn("sudo -S", source)
        self.assertNotIn("getpass", source)
        self.assertNotIn("--insecure", source)


if __name__ == "__main__":
    unittest.main()
