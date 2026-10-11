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

    def verification_failure(self, version):
        return None if self.verified else "fixture_verification_failure"


class QuickInstallTests(unittest.TestCase):
    def test_native_package_acceptance_diagnostics_are_filtered_and_bounded(self):
        manager = quick.NativeManager(quick.PROFILES["fedora-x86_64"])
        result = quick.subprocess.CompletedProcess(
            ["dnf"],
            0,
            "unrelated package output\n"
            ">>> Running %post scriptlet: synveil-0.1.0-1.x86_64\n"
            ">>> [synveil] applying sysusers (systemd-sysusers)\n"
            "systemd-tmpfiles: failed to create /var/lib/synveil\n"
            "private unrelated detail\n",
            "",
        )

        output = io.StringIO()
        with mock.patch.dict(os.environ, {"SYNVEIL_ACCEPTANCE_DIAGNOSTICS": "1"}):
            with contextlib.redirect_stderr(output):
                manager._emit_acceptance_package_diagnostics(result)
        diagnostics = output.getvalue()
        self.assertIn("NATIVE_PACKAGE_MANAGER_DIAGNOSTIC >>> Running %post scriptlet: synveil", diagnostics)
        self.assertIn("NATIVE_PACKAGE_MANAGER_DIAGNOSTIC systemd-tmpfiles:", diagnostics)
        self.assertNotIn("unrelated package output", diagnostics)
        self.assertNotIn("private unrelated detail", diagnostics)

        output = io.StringIO()
        with mock.patch.dict(os.environ, {}, clear=True):
            with contextlib.redirect_stderr(output):
                manager._emit_acceptance_package_diagnostics(result)
        self.assertEqual("", output.getvalue())

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

    def test_fedora_dnf_local_package_argument_is_positional_after_subcommand(self):
        manager = quick.NativeManager(quick.PROFILES["fedora-x86_64"])
        manager.artifact_evidence = dict(self.evidence, artifact_type="rpm")
        invoked = []

        def capture(command, *, requires_admin=False):
            invoked.append((command, requires_admin))
            return quick.subprocess.CompletedProcess(command, 0, "", "")

        with mock.patch.object(quick.os, "geteuid", return_value=1000), \
             mock.patch.object(quick, "require_staged_evidence"), \
             mock.patch.object(manager, "_run", side_effect=capture):
            manager.install(self.package, "0.2.0")

        self.assertEqual(
            [(["dnf", "install", "-y", str(self.package)], True)],
            invoked,
        )

    def test_fedora_authorization_denial_stays_an_authorization_failure(self):
        manager = quick.NativeManager(quick.PROFILES["fedora-x86_64"])
        manager.artifact_evidence = dict(self.evidence, artifact_type="rpm")
        denied = quick.subprocess.CompletedProcess(
            ["/usr/bin/dnf", "install", "-y", str(self.package)],
            1,
            "",
            "sudo: user is not allowed to execute /usr/bin/dnf",
        )
        with (
            mock.patch.object(quick.os, "geteuid", return_value=1000),
            mock.patch.object(quick, "system_executable", side_effect=lambda name: "/usr/bin/" + name),
            mock.patch.object(quick, "require_staged_evidence"),
            mock.patch.object(manager, "_run", return_value=denied),
            mock.patch.object(manager, "verify", return_value=False),
            mock.patch.object(manager, "installed_version", return_value=None),
            mock.patch.object(quick, "package_payload_present", return_value=False),
        ):
            with self.assertRaises(quick.QuickInstallError) as caught:
                manager.install(self.package, "0.2.0")
        self.assertEqual("AuthorizationFailed", caught.exception.category)
        self.assertEqual(quick.EXIT_AUTHORIZATION, caught.exception.exit_status)

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
        self.assertIn("fixture_verification_failure", error)

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

    def test_fedora_acceptance_authorizes_resolved_dnf_path_only(self):
        workflow = (ROOT / ".github/workflows/linux-packages.yml").read_text(encoding="utf-8")
        self.assertIn('DNF_REAL="$(readlink -f /usr/bin/dnf)"', workflow)
        self.assertIn("NOPASSWD: %s, %s -V synveil\\n", workflow)
        self.assertIn('"$DNF_REAL" "$RPM_REAL" > /etc/sudoers.d/synveil-installer', workflow)
        self.assertIn('sudo -n -- "$DNF_REAL" --version', workflow)
        self.assertNotIn("NOPASSWD: /usr/bin/dnf", workflow)

    def test_fedora_acceptance_authorizes_only_the_exact_rpm_verifier(self):
        workflow = (ROOT / ".github/workflows/linux-packages.yml").read_text(encoding="utf-8")
        self.assertIn('RPM_REAL="$(readlink -f /usr/bin/rpm)"', workflow)
        self.assertIn("NOPASSWD: %s, %s -V synveil\\n", workflow)
        self.assertIn('"$DNF_REAL" "$RPM_REAL" > /etc/sudoers.d/synveil-installer', workflow)
        self.assertNotIn("NOPASSWD: /usr/bin/rpm", workflow)

    def test_native_acceptance_reports_bounded_package_verifier_output(self):
        source = (ROOT / "scripts/run-linux-quick-install-acceptance.sh").read_text(encoding="utf-8")

        self.assertIn("report_native_package_state()", source)
        self.assertIn("dpkg --verify synveil", source)
        self.assertIn("rpm -V synveil", source)
        self.assertIn('sudo -- "$verifier" --verify synveil', source)
        self.assertIn('sudo -- "$verifier" -V synveil', source)
        self.assertIn('rpm -q --dump synveil', source)
        self.assertIn('stat -c \'NATIVE_PACKAGE_DIAGNOSTIC filesystem=', source)
        self.assertIn("sed -n '1,40p'", source)
        self.assertIn('exit "$quick_install_status"', source)

    def test_native_verification_failure_uses_bounded_reason_codes(self):
        manager = quick.NativeManager(quick.PROFILES["debian-x86_64"])
        with mock.patch.object(manager, "installed_version", return_value="0.2.0"), \
             mock.patch.object(quick.Path, "is_file", return_value=False):
            self.assertEqual(
                "required_payload_path_missing_or_not_regular",
                manager.verification_failure("0.2.0"),
            )

        with mock.patch.object(manager, "installed_version", return_value="0.2.0"), \
             mock.patch.object(quick, "REQUIRED_PATHS", ()), \
             mock.patch.object(quick, "system_executable", side_effect=lambda name: "/usr/bin/" + name), \
             mock.patch.object(quick.subprocess, "run", return_value=quick.subprocess.CompletedProcess(
                 ["dpkg", "--verify", "synveil"], 1, "./usr/bin/synveil-desktop\n", "")):
            self.assertEqual(
                "package_database_integrity_check_failed",
                manager.verification_failure("0.2.0"),
            )


class DpkgPolicyVerificationTests(unittest.TestCase):
    excluded_docs = "path-exclude=/usr/share/doc/*\npath-include=/usr/share/doc/*/copyright\n"

    def verification_failure(self, artifact_type, output, *, policy_excludes_docs=True, returncode=0):
        profile = quick.PROFILES["debian-x86_64" if artifact_type == "deb" else "fedora-x86_64"]
        manager = quick.NativeManager(profile)
        with tempfile.TemporaryDirectory() as directory:
            required = Path(directory) / "runtime-file"
            required.write_text("installed", encoding="utf-8")
            completed = quick.subprocess.CompletedProcess(["package verifier"], returncode, output, "")
            policy = self.excluded_docs if policy_excludes_docs else ""
            path_policy = lambda path: quick._dpkg_config_excludes_path(path, (policy,))
            with mock.patch.object(quick, "REQUIRED_PATHS", (str(required),)), \
                 mock.patch.object(manager, "installed_version", return_value="0.1.0"), \
                 mock.patch.object(manager, "_run", return_value=completed), \
                 mock.patch.object(quick, "_dpkg_path_is_policy_excluded", side_effect=path_policy):
                return manager.verification_failure("0.1.0")

    def test_dpkg_policy_parser_honors_ordered_path_include(self):
        license_path = "/usr/share/doc/synveil/LICENSE"
        copyright_path = "/usr/share/doc/synveil/copyright"
        re_included_license = self.excluded_docs + "path-include=/usr/share/doc/synveil/LICENSE\n"

        self.assertTrue(quick._dpkg_config_excludes_path(license_path, (self.excluded_docs,)))
        self.assertFalse(quick._dpkg_config_excludes_path(copyright_path, (self.excluded_docs,)))
        self.assertFalse(quick._dpkg_config_excludes_path(license_path, (re_included_license,)))

    def test_exact_policy_excluded_debian_notice_files_do_not_fail_integrity_check(self):
        output = (
            "missing     /usr/share/doc/synveil/LICENSE\n"
            "missing     /usr/share/doc/synveil/NOTICE\n"
        )

        self.assertIsNone(self.verification_failure("deb", output))

    def test_missing_notice_files_fail_without_matching_dpkg_policy(self):
        output = "missing     /usr/share/doc/synveil/LICENSE\n"

        self.assertEqual(
            "package_database_integrity_check_failed",
            self.verification_failure("deb", output, policy_excludes_docs=False),
        )

    def test_unexpected_missing_or_modified_files_still_fail_integrity_check(self):
        excluded_docs = "missing     /usr/share/doc/synveil/LICENSE\n"
        unexpected_runtime = "missing     /usr/bin/synveil-desktop\n"
        modified_notice = "??5??????   /usr/share/doc/synveil/LICENSE\n"

        self.assertEqual(
            "package_database_integrity_check_failed",
            self.verification_failure("deb", excluded_docs + unexpected_runtime),
        )
        self.assertEqual(
            "package_database_integrity_check_failed",
            self.verification_failure("deb", modified_notice),
        )

    def test_rpm_verification_output_remains_fail_closed(self):
        self.assertEqual(
            "package_database_integrity_check_failed",
            self.verification_failure("rpm", "missing /usr/share/doc/synveil/LICENSE\n"),
        )

    def test_permission_denied_verifier_result_retries_with_visible_authorization(self):
        with tempfile.TemporaryDirectory() as directory:
            runtime_file = Path(directory) / "runtime-file"
            runtime_file.write_text("installed", encoding="utf-8")
            for artifact_type, command, unprivileged_status in (
                ("deb", ["/usr/bin/dpkg", "--verify", "synveil"], 0),
                ("rpm", ["/usr/bin/rpm", "-V", "synveil"], 1),
            ):
                with self.subTest(artifact_type=artifact_type):
                    profile = quick.PROFILES[
                        "debian-x86_64" if artifact_type == "deb" else "fedora-x86_64"
                    ]
                    manager = quick.NativeManager(profile)
                    denied = quick.subprocess.CompletedProcess(
                        command,
                        unprivileged_status,
                        "......G..    /etc/synveil\n"
                        "missing     /etc/synveil/credentials (Permission denied)\n",
                        "",
                    )
                    clean = quick.subprocess.CompletedProcess(command, 0, "", "")
                    stderr = io.StringIO()
                    with (
                        mock.patch.object(manager, "installed_version", return_value="0.1.0"),
                        mock.patch.object(quick, "REQUIRED_PATHS", (str(runtime_file),)),
                        mock.patch.object(quick.os, "geteuid", return_value=1000),
                        mock.patch.object(quick, "system_executable", side_effect=lambda name: "/usr/bin/" + name),
                        mock.patch.object(quick.subprocess, "run", side_effect=(denied, clean)) as invoke,
                        contextlib.redirect_stderr(stderr),
                    ):
                        self.assertIsNone(manager.verification_failure("0.1.0"))

                    self.assertEqual(2, invoke.call_count)
                    first, second = (call.args[0] for call in invoke.call_args_list)
                    self.assertEqual(command, first)
                    self.assertEqual(["/usr/bin/sudo", "--", *command], second)
                    self.assertIn("Administrator authorization is needed", stderr.getvalue())

    def test_denied_authorization_does_not_become_integrity_pass_or_prompt_bypass(self):
        manager = quick.NativeManager(quick.PROFILES["fedora-x86_64"])
        command = ["/usr/bin/rpm", "-V", "synveil"]
        denied = quick.subprocess.CompletedProcess(
            command, 1, "missing /etc/synveil/credentials (Permission denied)\n", ""
        )
        sudo_denied = quick.subprocess.CompletedProcess(
            ["/usr/bin/sudo", "--", *command],
            1,
            "",
            "sudo: user is not allowed to execute /usr/bin/rpm -V synveil",
        )
        with tempfile.TemporaryDirectory() as directory:
            runtime_file = Path(directory) / "runtime-file"
            runtime_file.write_text("installed", encoding="utf-8")
            with (
                mock.patch.object(manager, "installed_version", return_value="0.1.0"),
                mock.patch.object(quick, "REQUIRED_PATHS", (str(runtime_file),)),
                mock.patch.object(quick.os, "geteuid", return_value=1000),
                mock.patch.object(quick, "system_executable", side_effect=lambda name: "/usr/bin/" + name),
                mock.patch.object(quick.subprocess, "run", side_effect=(denied, sudo_denied)),
                contextlib.redirect_stderr(io.StringIO()),
            ):
                with self.assertRaises(quick.QuickInstallError) as caught:
                    manager.verification_failure("0.1.0")
        self.assertEqual("AuthorizationFailed", caught.exception.category)
        self.assertEqual(quick.EXIT_AUTHORIZATION, caught.exception.exit_status)

    def test_privileged_verification_still_rejects_a_real_missing_file(self):
        manager = quick.NativeManager(quick.PROFILES["fedora-x86_64"])
        command = ["/usr/bin/rpm", "-V", "synveil"]
        denied = quick.subprocess.CompletedProcess(
            command, 1, "missing /etc/synveil/credentials (Permission denied)\n", ""
        )
        missing = quick.subprocess.CompletedProcess(
            ["/usr/bin/sudo", "--", *command], 1, "missing /usr/bin/synveil-client\n", ""
        )
        with tempfile.TemporaryDirectory() as directory:
            runtime_file = Path(directory) / "runtime-file"
            runtime_file.write_text("installed", encoding="utf-8")
            with (
                mock.patch.object(manager, "installed_version", return_value="0.1.0"),
                mock.patch.object(quick, "REQUIRED_PATHS", (str(runtime_file),)),
                mock.patch.object(quick.os, "geteuid", return_value=1000),
                mock.patch.object(quick, "system_executable", side_effect=lambda name: "/usr/bin/" + name),
                mock.patch.object(quick.subprocess, "run", side_effect=(denied, missing)),
                contextlib.redirect_stderr(io.StringIO()),
            ):
                self.assertEqual(
                    "package_database_integrity_check_failed",
                    manager.verification_failure("0.1.0"),
                )


if __name__ == "__main__":
    unittest.main()
