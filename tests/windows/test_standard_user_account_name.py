#!/usr/bin/env python3
"""Keep disposable local acceptance accounts within the Windows SAM limit."""

from __future__ import annotations

import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
RUNNER = ROOT / "scripts/invoke-windows-standard-user-test.ps1"
PER_USER_TEST = ROOT / "scripts/test-windows-per-user-installation.ps1"
WINDOWS_INSTALLER = ROOT / "scripts/build-windows-installer.ps1"
DESKTOP_BRIDGE = ROOT / "crates/desktop/src/bridge.rs"


class StandardUserAccountNameTests(unittest.TestCase):
    def test_generated_name_fits_windows_local_account_limit(self) -> None:
        source = RUNNER.read_text(encoding="utf-8")
        match = re.search(
            r"(?m)^\$user\s*=\s*'([^']+)'\s*\+\s*\[Guid\]::NewGuid\(\)"
            r"\.ToString\('N'\)\.Substring\(0,(\d+)\)\s*$",
            source,
        )
        self.assertIsNotNone(match, "disposable account name must use the reviewed generated form")
        assert match is not None
        prefix, suffix_length_text = match.groups()
        suffix_length = int(suffix_length_text)
        self.assertRegex(prefix, r"^[A-Za-z0-9_]+$")
        self.assertGreater(suffix_length, 0)
        self.assertLessEqual(
            len(prefix) + suffix_length,
            20,
            "Windows local SAM account names cannot exceed 20 characters",
        )

    def test_desktop_known_folder_has_a_profile_fallback(self) -> None:
        source = PER_USER_TEST.read_text(encoding="utf-8")
        self.assertIn(
            "$desktopFolder = [Environment]::GetFolderPath([Environment+SpecialFolder]::Desktop)",
            source,
        )
        self.assertIn("if ([string]::IsNullOrWhiteSpace($desktopFolder))", source)
        self.assertIn("$desktopFolder = Join-Path $profileRoot 'Desktop'", source)
        self.assertIn("New-Item -Path $desktopFolder -ItemType Directory -Force", source)
        self.assertIn("$desktop = Join-Path $desktopFolder 'Synveil.lnk'", source)
        self.assertNotIn("Join-Path ([Environment]::GetFolderPath('Desktop'))", source)

    def test_standard_user_smoke_uses_packaged_windows_qpa_plugin(self) -> None:
        source = PER_USER_TEST.read_text(encoding="utf-8")
        installer = WINDOWS_INSTALLER.read_text(encoding="utf-8")

        self.assertIn("$env:QT_QPA_PLATFORM='windows'", source)
        self.assertNotIn("QT_QPA_PLATFORM='offscreen'", source)
        self.assertIn("'platforms/qwindows.dll'", installer)

    def test_qml_smoke_does_not_leave_a_background_client_running(self) -> None:
        bridge = DESKTOP_BRIDGE.read_text(encoding="utf-8")
        start = bridge.index("fn start_controller(mut self: Pin<&mut Self>)")
        end = bridge.index("fn install_tray(mut self: Pin<&mut Self>)", start)
        controller = bridge[start:end]
        self.assertIn("fn should_start_background_client(smoke_test: bool)", bridge)
        self.assertLess(
            controller.index("if !should_start_background_client(smoke_test)"),
            controller.index("launch_manager.ensure_running().await"),
        )
        self.assertIn("qml_smoke_test_does_not_start_detached_client", bridge)

    def test_client_probe_uses_an_isolated_malformed_config_fixture(self) -> None:
        source = PER_USER_TEST.read_text(encoding="utf-8")
        fixture = source.index("$clientProbeConfig =")
        probe = source.index("78 'client probe'")
        self.assertLess(fixture, probe)
        self.assertIn("'profile_id=not-a-profile'", source[fixture:probe])
        self.assertIn("$env:SYNVEIL_CLIENT_CONFIG = $clientProbeConfig", source[fixture:probe])
        self.assertIn("$previousClientConfig", source[fixture:])
        self.assertIn("Remove-Item Env:SYNVEIL_CLIENT_CONFIG", source[fixture:])

    def test_installed_runtime_verifier_uses_powershell_failure_semantics(self) -> None:
        source = PER_USER_TEST.read_text(encoding="utf-8")
        invocation = source.index("test-windows-installed-runtime.ps1")
        check_start = source.rindex("try {", 0, invocation)
        check_end = source.index("$acl = Get-Acl", invocation)
        check = source[check_start:check_end]

        self.assertIn("try {", check)
        self.assertIn("catch {", check)
        self.assertIn("PER_USER_RUNTIME_FAILURE: installed manifest verifier failed:", check)
        self.assertNotIn("$LASTEXITCODE", check)

    def test_uninstall_waits_boundedly_for_inno_self_cleanup(self) -> None:
        source = PER_USER_TEST.read_text(encoding="utf-8")
        start = source.index("function Wait-ForPackageRootRemoval")
        end = source.index("\n}", start) + 2
        wait = source[start:end]

        self.assertIn("AddSeconds(30)", wait)
        self.assertIn("Start-Sleep -Milliseconds 100", wait)
        self.assertIn("package root remains after bounded cleanup wait", wait)
        self.assertIn("Inno Setup may return from the main uninstaller", wait)
        first_uninstall = source.index("'uninstall.log'", source.index("Invoke-Bounded $uninstaller"))
        first_wait = source.index("Wait-ForPackageRootRemoval 'first uninstall'")
        second_uninstall_call = source.index("'second uninstaller'", first_wait)
        second_wait = source.index("Wait-ForPackageRootRemoval 'second uninstall'")
        self.assertLess(first_uninstall, first_wait)
        self.assertLess(first_wait, second_uninstall_call)
        self.assertLess(second_uninstall_call, second_wait)

    def test_standard_user_failure_emits_bounded_redacted_uninstall_diagnostics(self) -> None:
        source = RUNNER.read_text(encoding="utf-8")
        start = source.index("if ($null -ne $primaryFailure) {", source.index("$childLogs ="))
        end = source.index("if ($accountCreationAttempted)", start)
        diagnostics = source[start:end]

        self.assertIn("(?:PER_USER|LIFECYCLE)_[A-Z0-9_]+:", diagnostics)
        self.assertIn(r"LIFECYCLE_TIMEOUT_(?:PROCESS|RECOVERY)\b", diagnostics)
        self.assertIn("Select-Object -First 24", diagnostics)
        self.assertIn("Select-Object -First 20", diagnostics)
        self.assertIn("Select-Object -First 40", diagnostics)
        self.assertIn("repair-production.log", diagnostics)
        self.assertIn("Get-Content -LiteralPath $repairLog -Tail 24", diagnostics)
        self.assertIn("Win32_Process", diagnostics)
        self.assertIn("GetOwner", diagnostics)
        self.assertIn("Select-Object -First 12", diagnostics)
        self.assertIn("[USER_PROFILE]", diagnostics)
        self.assertIn("STANDARD_USER_DIAGNOSTIC package_root_entries=", diagnostics)

    def test_lifecycle_child_failure_emits_bounded_redacted_exception_diagnostics(self) -> None:
        source = RUNNER.read_text(encoding="utf-8")
        lifecycle = (ROOT / "scripts/test-windows-installer-lifecycle.ps1").read_text(encoding="utf-8")
        self.assertIn("$ErrorView = 'NormalView'", lifecycle)
        self.assertIn("$lifecycleStdout = Join-Path $EvidenceDirectory 'lifecycle.stdout.log'", source)
        self.assertIn("$lifecycleStderr = Join-Path $EvidenceDirectory 'lifecycle.stderr.log'", source)
        self.assertIn("@($stdout,$stderr,$lifecycleStdout,$lifecycleStderr)", source)
        self.assertIn("PER_USER|LIFECYCLE", source)
        self.assertIn("uninstaller|reinstaller|desktop smoke|client probe", source)
        self.assertIn(r"LIFECYCLE_TIMEOUT_(?:PROCESS|RECOVERY)\b", source)
        self.assertIn("Select-Object -First 24", source)
        self.assertIn("$safeLine.Replace(\"$env:SystemDrive\\Users\\$user\", '[USER_PROFILE]')", source)
        self.assertIn("$safeLine.Replace($password,'[REDACTED]')", source)

    def test_lifecycle_reconstructs_standard_user_profile_environment_before_path_use(self) -> None:
        lifecycle = (ROOT / "scripts/test-windows-installer-lifecycle.ps1").read_text(encoding="utf-8")
        profile_setup = lifecycle[: lifecycle.index("$root = Join-Path $env:LOCALAPPDATA")]

        for marker in (
            "$identity = [Security.Principal.WindowsIdentity]::GetCurrent()",
            "LIFECYCLE_PROFILE_FAILURE: standard-user profile path is unavailable",
            "LIFECYCLE_PROFILE_FAILURE: profile path does not match the standard-user identity",
            "$env:USERPROFILE = $profileRoot",
            "$env:LOCALAPPDATA = $localAppData",
            "$env:APPDATA = $roamingAppData",
            "$env:TEMP = $userTemp",
            "$env:TMP = $userTemp",
            "$desktopFolder = Join-Path $profileRoot 'Desktop'",
        ):
            self.assertIn(marker, profile_setup)

    def test_lifecycle_timeout_records_and_terminates_setup_process_tree(self) -> None:
        lifecycle = (ROOT / "scripts/test-windows-installer-lifecycle.ps1").read_text(encoding="utf-8")
        timeout = lifecycle[lifecycle.index("if (!$process.WaitForExit(120000))") : lifecycle.index("Assert-True ($process.ExitCode")]

        self.assertIn("Get-SetupProcessTree $process.Id", timeout)
        self.assertIn("LIFECYCLE_TIMEOUT_PROCESS name=", timeout)
        self.assertIn("$process.Kill($true)", timeout)
        self.assertIn("WaitForExit(30000)", timeout)
        self.assertIn("process_tree_terminated=$treeTerminated", timeout)
        self.assertIn('throw "LIFECYCLE_TIMEOUT: $Name', timeout)

    def test_standard_user_cleanup_result_updates_the_failure_record(self) -> None:
        source = RUNNER.read_text(encoding="utf-8")
        start = source.index("$recordPath = if", source.index("if ($sid)"))
        cleanup = source[start : source.index("$password = $null", start)]

        self.assertIn("$null -ne $primaryFailure", cleanup)
        self.assertIn("$failureEvidence", cleanup)
        self.assertIn("$record.cleanup =", cleanup)
        self.assertIn("-Name runner_cleanup", cleanup)
        self.assertIn("-Name runner_cleanup_error", cleanup)


if __name__ == "__main__":
    unittest.main()
