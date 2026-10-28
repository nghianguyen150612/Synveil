#!/usr/bin/env python3
"""Network-free validation of the deliberately fail-closed P028 checkpoint."""
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / ".github/workflows/windows-native-acceptance.yml"
DOC = ROOT / "docs/v0.2/WINDOWS_NATIVE_ACCEPTANCE.md"
MANIFEST = ROOT / "docs/v0.2/PROMPT028_MANIFEST.md"
EVIDENCE_VALIDATOR = ROOT / "scripts/validate-windows-native-evidence.py"
EVIDENCE_TESTS = ROOT / "scripts/test_windows_native_evidence.py"
ENVIRONMENT_GATE = ROOT / "scripts/qualify-windows-native-environment.ps1"
GUI_HARNESS = ROOT / "scripts/test-windows-gui-journey.ps1"
UNINSTALL_HARNESS = ROOT / "scripts/test-windows-installed-uninstall.ps1"
IPC_PROBE = ROOT / "crates/client/examples/p013_windows_control_probe.rs"
LOGON_COLLECTOR = ROOT / "scripts/collect-windows-new-logon-evidence.ps1"
LOGOFF_COLLECTOR = ROOT / "scripts/collect-windows-logoff-evidence.ps1"
PROVISION_USER = ROOT / "scripts/provision-windows-p013-test-user.ps1"
CLEANUP_USER = ROOT / "scripts/cleanup-windows-p013-test-user.ps1"
SUMMARY_SCRIPT = ROOT / "scripts/summarize-windows-native-acceptance.py"


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def main() -> None:
    workflow = WORKFLOW.read_text(encoding="utf-8")
    docs = DOC.read_text(encoding="utf-8") + MANIFEST.read_text(encoding="utf-8")
    for token in (
        "windows-2025", "build-windows-candidate", "standard-user-native",
        "SynveilSetup.exe", "p028-candidate.json", "Get-FileHash",
        "source_commit", "invoke-windows-standard-user-test.ps1",
        "requestedExecutionLevel", "asInvoker", "BLOCKED_BY_ENVIRONMENT",
        "interactive_gui='BLOCKED'", "real_logon='BLOCKED'", "named_pipe='BLOCKED'",
        "if: always()", "timeout-minutes", "diagnostics_redacted",
    ):
        require(token in workflow, f"missing checkpoint wiring: {token}")
    require(workflow.count("build-windows-installer.ps1") == 2,
            "candidate must be built exactly twice only for reproducibility")
    require("actions/download-artifact@v4" in workflow and
            "identity.sha256" in workflow and "GITHUB_SHA" in workflow,
            "consumer must authenticate transferred bytes and source")
    require("$setup = 'target/release-candidate/SynveilSetup.exe'" in workflow and
            workflow.index("Record exact Setup signature state") <
            workflow.index("Verify and stage the exact assembled release candidate"),
            "manual candidate signature check must inspect the downloaded Setup before staging")
    require("$candidateRoot = Join-Path $PWD 'candidate/windows-installer'" in workflow and
            "Join-Path $candidateRoot 'p028-candidate.json'" in workflow and
            "Join-Path $candidateRoot 'SynveilSetup.exe'" in workflow and
            "Join-Path $candidateRoot 'SYNVEIL-RELEASE-MANIFEST.json'" in workflow and
            "candidate/windows-installer/SynveilSetup.exe" in workflow,
            "consumer paths must preserve the uploaded windows-installer directory")
    for evidence in (
        "release_manifest_sha256", "P028_CANDIDATE_MANIFEST_DIGEST_FAILURE",
        "P028_CANDIDATE_MANIFEST_IDENTITY_FAILURE",
        "P028_CANDIDATE_MANIFEST_ARTIFACT_FAILURE", "actualSize",
    ):
        require(evidence in workflow, f"transferred candidate provenance: {evidence}")
    require("SYNVEIL_V0_2_WINDOWS_INSTALL_EXPERIENCE_READY" not in workflow + docs,
            "readiness marker must remain withheld before native evidence")
    evidence_validator = EVIDENCE_VALIDATOR.read_text(encoding="utf-8")
    evidence_tests = EVIDENCE_TESTS.read_text(encoding="utf-8")
    environment_gate = ENVIRONMENT_GATE.read_text(encoding="utf-8")
    gui_harness = GUI_HARNESS.read_text(encoding="utf-8")
    uninstall_harness = UNINSTALL_HARNESS.read_text(encoding="utf-8")
    ipc_probe = IPC_PROBE.read_text(encoding="utf-8")
    logon_collector = LOGON_COLLECTOR.read_text(encoding="utf-8")
    logoff_collector = LOGOFF_COLLECTOR.read_text(encoding="utf-8")
    provision_user = PROVISION_USER.read_text(encoding="utf-8")
    cleanup_user = CLEANUP_USER.read_text(encoding="utf-8")
    summary_script = SUMMARY_SCRIPT.read_text(encoding="utf-8")
    for token in (
        "BLOCKED_BY_ENVIRONMENT", "NOT_RUN", "INCOMPLETE", "REQUIRED_CHECKS",
        "qualified_isolated_native", "security_4624_matches_current_logon_luid",
        "task_logon_trigger_event", "pipe_wrong_user", "pipe_unsupported_version",
        "pipe_oversized_payload", "pipe_malformed_message", "no_tcp_fallback",
        "independently verified cleanup", "release_manifest_sha256",
        "component_evidence_sha256", "harness_source_commit", "PASS requires every component evidence digest",
    ):
        require(token in evidence_validator, f"missing fail-closed evidence contract: {token}")
    for token in (
        "github_hosted_service", "alternate_credentials_noninteractive", "interactive_session",
        "test_manual_task_run_is_not_logon_trigger_evidence",
        "test_each_required_surface_is_independently_fail_closed",
        "test_raw_pipe_payloads_are_forbidden", "test_wizard_execution_must_remain_disabled",
        "test_harness_revision_commit_is_bound_separately_from_candidate_source",
        "test_summary_rejects_components_from_a_different_harness_commit",
    ):
        require(token in evidence_tests, f"missing native-evidence regression: {token}")
    for token in (
        "I_APPROVE_DISPOSABLE_SYNVEIL_WINDOWS_ACCEPTANCE", "GITHUB_ACTIONS",
        "OpenInputDesktop", "WTSGetActiveConsoleSessionId", "WTSQuerySessionInformation",
        "ProcessIdToSessionId", "ExpectedSetupSha256", "ExpectedManifestSha256",
        "P013_CANDIDATE_IDENTITY_MISMATCH", "operator_approved = $true",
    ):
        require(token in environment_gate, f"missing native environment check: {token}")
    for forbidden in ("Start-Process", "Register-ScheduledTask", "New-LocalUser", "Remove-LocalUser", "schtasks.exe"):
        require(forbidden not in environment_gate, f"environment qualification must not mutate machine state: {forbidden}")
    require("test_windows_native_evidence.py" in workflow,
            "hosted source-contract job must validate the evidence contract without claiming native acceptance")
    require("cargo test -p synveil-client --test desktop_control_ipc --locked" in workflow,
            "hosted Windows native target must execute the existing IPC regression suite")
    require("Parse PowerShell acceptance tools without running them" in workflow and
            "P013_POWERSHELL_PARSE_FAILURE" in workflow,
            "Windows source job must parse PowerShell tools without running native acceptance")
    for token in (
        "I_APPROVE_DISPOSABLE_SYNVEIL_WINDOWS_ACCEPTANCE", "UIAutomationClient", "OpenInputDesktop",
        "ExpectedSetupSha256", "ExpectedManifestSha256", "Start-Process -FilePath $setupPath",
        "ExpectedHarnessCommit",
        "Ready to Install", "Installing Synveil", "Completing the Synveil Setup Wizard",
        "Start Synveil when I sign in", "Create a desktop shortcut", "Open Synveil after installation",
        "Welcome to Synveil", "P013_GUI_USER_OPTIONS_MISMATCH", "P013_DESKTOP_SHUTDOWN_TIMEOUT",
        "Invoke-ControlIpcProbe", "p013_windows_control_probe", "Get-NetTCPConnection",
        "P013_INSTALLED_CLIENT_USER_MISMATCH", "CARGO_TARGET_DIR",
        "IsProcessElevated", "installed_desktop_ipc_ping",
        "debug') 'examples'",
        "pipe_server_termination", "pipe_expired_endpoint", "pipe_restart", "pipe_stale_profile",
        "Invoke-WrongUserControlProbe", "P013_DISPOSABLE_TEST_USER_REQUIRED", "I_APPROVE_DISPOSABLE_SYNVEIL_WINDOWS_ACCEPTANCE",
        "ui_evidence", "screenshots_status='NOT_CAPTURED'", "P013_UNSUPPORTED_ARCHITECTURE",
        "Write-BlockedGuiEvidence",
        "result='BLOCKED_BY_ENVIRONMENT'", "exit 2",
    ):
        require(token in gui_harness, f"missing interactive UI journey assertion: {token}")
    require(re.search(
        r"if\s*\(\$env:GITHUB_ACTIONS\s*-eq\s*'true'.*?Write-BlockedGuiEvidence.*?exit 2",
        gui_harness, re.S,
    ) is not None, "hosted/noninteractive GUI harness must emit blocked evidence before product execution")
    require("Start-Process -FilePath $setupPath -PassThru" in gui_harness and
            re.search(r"Start-Process\s+-FilePath\s+\$setupPath[^\n]*(?:/VERYSILENT|/SILENT)", gui_harness) is None,
            "GUI acceptance must launch the visible Setup wizard")
    require("-NonInteractive" not in gui_harness,
            "GUI acceptance cannot substitute noninteractive execution")
    require("-Credential $credential" in gui_harness and "pipe_wrong_user" in gui_harness,
            "cross-user IPC must use an ephemeral alternate-user credential only for the denial probe")
    require("-Verb RunAs" not in gui_harness and "-Verb RunAs" not in environment_gate,
            "acceptance harnesses cannot request product-process elevation")
    require("Get-OwnedRegisteredUninstaller" not in gui_harness and "P013_UNINSTALL_STATE_NOT_PRESERVED" not in gui_harness,
            "GUI/logon evidence must keep the installed candidate present until the later uninstall phase")
    for token in (
        "ExpectedSourceCommit", "ExpectedHarnessCommit", "Get-OwnedRegisteredUninstaller",
        "client_protocol_shutdown", "P013_UNINSTALL_STATE_NOT_PRESERVED", "Get-HklmRegistrationCount",
        "P013_UNINSTALLER_ELEVATED", "uninstall_preservation", "probe_temp_cleanup",
        "test-windows-installed-runtime.ps1",
    ):
        require(token in uninstall_harness, f"missing post-logoff uninstall acceptance: {token}")
    require("/VERYSILENT" in uninstall_harness and "Start-Process -FilePath $setupPath" not in uninstall_harness,
            "quiet uninstall can run only after visible Setup acceptance; it cannot substitute for the GUI journey")
    for token in (
        "DesktopClientConfig::existing_profile_id", "DesktopControlEndpoint::for_profile",
        "DesktopControlClient::connect", "client.ping()", "u16::MAX", "pipe_unsupported_version",
        "pipe_malformed_message", "pipe_oversized_payload", "pipe_duplicate_connection",
        "pipe_timeout_cancellation", "pipe_wrong_user", "pipe_server_termination", "pipe_expired_endpoint",
        "pipe_stale_profile", "owner_ping", "ErrorKind::PermissionDenied",
        "DESKTOP_CONTROL_MAX_FRAME_BYTES",
    ):
        require(token in ipc_probe, f"missing installed named-pipe acceptance probe: {token}")
    for forbidden in ("DesktopControlServer::bind", "MockControlServer", "TcpListener", "\\\\.\\pipe\\synveil-"):
        require(forbidden not in ipc_probe, f"acceptance probe cannot create a mock/guessed endpoint: {forbidden}")
    for token in (
        "Security';Id=4624", "TargetLogonId", "TargetUserSid", "LogonType",
        "Microsoft-Windows-TaskScheduler/Operational", "Id=100,107,110",
        "NotBeforeUtc", "Export-ScheduledTask", "LogonTrigger", "InteractiveToken",
        "LeastPrivilege", "logoff_shutdown='NOT_RUN'", "P013_OPERATOR_APPROVAL_REQUIRED",
        "scheduler_manual_run_absent", "P013_MANUAL_TASK_RUN_CANNOT_PROVE_LOGON_TRIGGER",
    ):
        require(token in logon_collector, f"missing read-only genuine-logon evidence: {token}")
    for forbidden in ("Register-ScheduledTask", "Unregister-ScheduledTask", "schtasks.exe", "New-LocalUser", "Remove-LocalUser", "Start-Process"):
        require(forbidden not in logon_collector, f"logon evidence collection cannot mutate state or run tasks: {forbidden}")
    for token in ("TargetLogonId", "TargetUserSid", "Id=4634", "Id=100,102",
                  "authentication_luid_sha256", "P013_CLIENT_REMAINS_AFTER_LOGOFF",
                  "raw_logon_ids_recorded=$false", "P013_OPERATOR_APPROVAL_REQUIRED"):
        require(token in logoff_collector, f"missing real logoff and client shutdown correlation: {token}")
    for forbidden in ("Start-Process", "Register-ScheduledTask", "Unregister-ScheduledTask", "schtasks.exe", "New-LocalUser", "Remove-LocalUser"):
        require(forbidden not in logoff_collector, f"logoff collector cannot run or mutate the product: {forbidden}")
    for token in ("New-LocalUser", "Read-Host -AsSecureString", "SYNVEIL P013 DISPOSABLE ACCEPTANCE ACCOUNT",
                  "P013_ACCOUNT_PROVISIONING_REQUIRES_SEPARATE_ADMIN_PHASE"):
        require(token in provision_user, f"missing explicit disposable-user provisioning guarantee: {token}")
    for token in ("Remove-LocalUser", "Remove-CimInstance", "Unregister-ScheduledTask", "independent_verification",
                  "P013_INDEPENDENT_CLEANUP_VERIFICATION_FAILURE", "P013_OPERATOR_APPROVAL_REQUIRED"):
        require(token in cleanup_user, f"missing scoped cleanup and independent verification: {token}")
    for forbidden in ("Get-Credential", "ConvertTo-SecureString", "Set-LocalUser", "Add-LocalGroupMember"):
        require(forbidden not in cleanup_user, f"cleanup must not create or persist credentials: {forbidden}")
    require("if ($accounts.Count -eq 1)" in cleanup_user and "if ($profiles.Count -eq 1)" in cleanup_user and
            "remainingAccount.Count -ne 0" in cleanup_user,
            "cleanup must tolerate already-removed state and verify it independently")
    for token in ("current_source_matches", "verify_candidate", "component_candidate_matches",
                  "component_harness_digest", "component_evidence_sha256", "harness_source_commit",
                  "--harness-commit", "windows-uninstall-preservation-v1.json", '"NOT_RUN"',
                  "validate(summary)"):
        require(token in summary_script, f"missing fail-closed evidence aggregation: {token}")
    for status in ("SOURCE_PRESENT", "STATIC_VERIFIED", "CI_NATIVE_SCOPED", "NATIVE_CLEAN_MACHINE", "BLOCKED", "FAIL", "PASS"):
        require(status in docs, f"missing evidence terminology: {status}")
    print("windows native acceptance contract: PASS (checkpoint remains blocked)")


if __name__ == "__main__":
    main()
