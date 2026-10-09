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

    def execute(self, *, evidence=None, acquire_error=None, required_paths=None):
        release = self.evidence if evidence is None else evidence
        paths = quick.REQUIRED_PATHS if required_paths is None else required_paths
        output, error = io.StringIO(), io.StringIO()
        detected = quick.linux_platform_detection.DetectionResult(
            os_id="ubuntu", version_id="24.04", architecture="x86_64", qualification_status="QUALIFIED",
            target_id="ubuntu-24.04-x86_64", profile="debian-x86_64", artifact_type="deb",
            package_manager="APT", reason_code="qualified_exact_policy_match")
        acquisition_patch = (mock.patch.object(quick, "acquire", side_effect=acquire_error)
                             if acquire_error is not None
                             else mock.patch.object(quick, "acquire", return_value=release))
        with acquisition_patch, \
             mock.patch.object(quick, "REQUIRED_PATHS", paths), \
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

    def test_acquisition_disk_full_is_actionable_before_native_mutation(self):
        error = quick.release_download.AcquisitionError("INSUFFICIENT_DISK_SPACE", "full")
        status, _, message = self.execute(acquire_error=error)
        self.assertEqual(quick.EXIT_DISK_SPACE, status)
        self.assertEqual([], FakeManager.install_calls)
        self.assertIn("InsufficientDiskSpace", message)
        self.assertIn("No package change was made", message)

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

    def test_native_process_interruption_requires_inspection_and_never_replays(self):
        manager = quick.NativeManager(quick.PROFILES["debian-x86_64"])
        manager.artifact_evidence = self.evidence
        with mock.patch.object(quick.os, "geteuid", return_value=1000), \
             mock.patch.object(quick, "system_executable", side_effect=lambda name: "/usr/bin/" + name), \
             mock.patch.object(quick.subprocess, "run", side_effect=KeyboardInterrupt) as invoke:
            with self.assertRaises(quick.QuickInstallError) as caught:
                manager.install(self.package, "0.2.0")
        self.assertEqual("OutcomeUnknown", caught.exception.category)
        self.assertIn("Inspect native package state", caught.exception.action)
        invoke.assert_called_once()

    def test_native_install_uses_each_managers_supported_argument_contract(self):
        for profile, expected in (("debian-x86_64", ["apt-get", "install", "-y", "--"]),
                                  ("fedora-x86_64", ["dnf", "install", "-y"])):
            with self.subTest(profile=profile):
                manager = quick.NativeManager(quick.PROFILES[profile])
                manager.artifact_evidence = self.evidence
                with mock.patch.object(manager, "_run", return_value=mock.Mock(returncode=0, stdout="", stderr="")) as invoke:
                    manager.install(self.package, "0.2.0")
                invoke.assert_called_once_with([*expected, str(self.package)], mutate=True)

    def test_dnf_operand_stays_a_discrete_absolute_path(self):
        package = self.root / "package --verbose $(not-a-command).rpm"
        package.write_bytes(self.package.read_bytes())
        manager = quick.NativeManager(quick.PROFILES["fedora-x86_64"])
        manager.artifact_evidence = dict(self.evidence, final_path=str(package))
        with mock.patch.object(manager, "_run", return_value=mock.Mock(returncode=0, stdout="", stderr="")) as invoke:
            manager.install(package, "0.2.0")
        invoke.assert_called_once_with(["dnf", "install", "-y", str(package)], mutate=True)

    def test_partial_payload_with_absent_package_metadata_stops_before_retry(self):
        partial = self.root / "partial-owned-payload"
        partial.write_bytes(b"interrupted package copy")
        status, _, error = self.execute(required_paths=(str(partial),))
        self.assertEqual(quick.EXIT_UNKNOWN, status)
        self.assertEqual([], FakeManager.install_calls)
        self.assertIn("OutcomeUnknown", error)
        self.assertIn("Package state may have changed", error)
        self.assertIn("Inspect or repair partial native package state", error)
        self.assertNotIn("No package change was made", error)

    def test_post_install_verification_required(self):
        FakeManager.verified = False
        status, _, error = self.execute()
        self.assertEqual(quick.EXIT_VERIFICATION, status)
        self.assertIn("VerificationFailed", error)

    def test_package_verifier_allows_only_expected_policy_and_visibility_records(self):
        self.assertTrue(quick.package_verification_is_clean(
            0,
            "missing     /etc/synveil/credentials (Permission denied)\n"
            "missing     /usr/share/doc/synveil/LICENSE\n"
            "missing     /usr/share/doc/synveil/NOTICE\n",
        ))
        self.assertFalse(quick.package_verification_is_clean(
            0, "missing     /usr/bin/synveil-client\n"
        ))
        self.assertFalse(quick.package_verification_is_clean(
            0, "??5??????   /usr/share/doc/synveil/LICENSE\n"
        ))
        self.assertFalse(quick.package_verification_is_clean(
            0, "missing     /etc/synveil/credentials/secret (Permission denied)\n"
        ))
        self.assertFalse(quick.package_verification_is_clean(
            1, "missing     /usr/share/doc/synveil/LICENSE\n"
        ))

    def test_explicit_profile_cannot_override_detection(self):
        detected = quick.linux_platform_detection.DetectionResult(
            os_id="fedora", version_id="42", architecture="x86_64", qualification_status="QUALIFIED",
            target_id="fedora-42-x86_64", profile="fedora-x86_64", artifact_type="rpm",
            package_manager="DNF", reason_code="qualified_exact_policy_match")
        with self.assertRaises(quick.QuickInstallError) as caught:
            quick.resolve_profile("debian-x86_64", detected)
        self.assertEqual("UnsupportedPlatform", caught.exception.category)

    def test_rpm_permission_boundary_is_exact_and_other_failures_stay_closed(self):
        protected = "missing     /etc/synveil/credentials (Permission denied)\n"
        self.assertTrue(quick.package_verification_is_clean(1, protected, artifact_type="rpm"))
        for status, output, stderr in (
                (1, "", ""), (2, protected, ""), (1, protected, "rpm database error"),
                (1, "missing /etc/synveil/credentials\n", ""),
                (1, "missing /etc/synveil/credentials/token (Permission denied)\n", ""),
                (1, "missing /usr/share/doc/synveil/LICENSE\n", ""),
                (1, protected + "......G.. /etc/synveil\n", ""),
                (1, protected + "S.5...... /usr/bin/synveil-client\n", ""),
                (1, protected + "missing /usr/bin/synveil-desktop\n", "")):
            with self.subTest(status=status, output=output, stderr=stderr):
                self.assertFalse(quick.package_verification_is_clean(
                    status, output, artifact_type="rpm", stderr=stderr))
        self.assertFalse(quick.package_verification_is_clean(1, protected, artifact_type="deb"))

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
