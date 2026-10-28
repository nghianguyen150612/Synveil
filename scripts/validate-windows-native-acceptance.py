#!/usr/bin/env python3
"""Network-free validation of the deliberately fail-closed P028 checkpoint."""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / ".github/workflows/windows-native-acceptance.yml"
DOC = ROOT / "docs/v0.2/WINDOWS_NATIVE_ACCEPTANCE.md"
MANIFEST = ROOT / "docs/v0.2/PROMPT028_MANIFEST.md"
EVIDENCE_VALIDATOR = ROOT / "scripts/validate-windows-native-evidence.py"
EVIDENCE_TESTS = ROOT / "scripts/test_windows_native_evidence.py"
ENVIRONMENT_GATE = ROOT / "scripts/qualify-windows-native-environment.ps1"
GUI_HARNESS = ROOT / "scripts/test-windows-gui-journey.ps1"


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
    for token in (
        "BLOCKED_BY_ENVIRONMENT", "NOT_RUN", "INCOMPLETE", "REQUIRED_CHECKS",
        "qualified_isolated_native", "security_4624_matches_current_logon_luid",
        "task_logon_trigger_event", "pipe_wrong_user", "pipe_unsupported_version",
        "pipe_oversized_payload", "pipe_malformed_message", "no_tcp_fallback",
        "independently verified cleanup", "release_manifest_sha256",
    ):
        require(token in evidence_validator, f"missing fail-closed evidence contract: {token}")
    for token in (
        "github_hosted_service", "alternate_credentials_noninteractive", "interactive_session",
        "test_manual_task_run_is_not_logon_trigger_evidence",
        "test_each_required_surface_is_independently_fail_closed",
        "test_raw_pipe_payloads_are_forbidden", "test_wizard_execution_must_remain_disabled",
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
    require("Parse PowerShell acceptance tools without running them" in workflow and
            "P013_POWERSHELL_PARSE_FAILURE" in workflow,
            "Windows source job must parse PowerShell tools without running native acceptance")
    for token in (
        "I_APPROVE_DISPOSABLE_SYNVEIL_WINDOWS_ACCEPTANCE", "UIAutomationClient", "OpenInputDesktop",
        "ExpectedSetupSha256", "ExpectedManifestSha256", "Start-Process -FilePath $setupPath",
        "Ready to Install", "Installing Synveil", "Completing the Synveil Setup Wizard",
        "Start Synveil when I sign in", "Create a desktop shortcut", "Open Synveil after installation",
        "Welcome to Synveil", "P013_GUI_USER_OPTIONS_MISMATCH", "P013_DESKTOP_SHUTDOWN_TIMEOUT",
    ):
        require(token in gui_harness, f"missing interactive UI journey assertion: {token}")
    for forbidden in ("/VERYSILENT", "/SILENT", "-NonInteractive", "-Credential"):
        require(forbidden not in gui_harness, f"GUI acceptance cannot substitute silent/alternate-user execution: {forbidden}")
    for status in ("SOURCE_PRESENT", "STATIC_VERIFIED", "CI_NATIVE_SCOPED", "NATIVE_CLEAN_MACHINE", "BLOCKED", "FAIL", "PASS"):
        require(status in docs, f"missing evidence terminology: {status}")
    print("windows native acceptance contract: PASS (checkpoint remains blocked)")


if __name__ == "__main__":
    main()
