#!/usr/bin/env python3
"""Network-free validation of the deliberately fail-closed P028 checkpoint."""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / ".github/workflows/windows-native-acceptance.yml"
DOC = ROOT / "docs/v0.2/WINDOWS_NATIVE_ACCEPTANCE.md"
MANIFEST = ROOT / "docs/v0.2/PROMPT028_MANIFEST.md"


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
    for status in ("SOURCE_PRESENT", "STATIC_VERIFIED", "CI_NATIVE_SCOPED", "NATIVE_CLEAN_MACHINE", "BLOCKED", "FAIL", "PASS"):
        require(status in docs, f"missing evidence terminology: {status}")
    print("windows native acceptance contract: PASS (checkpoint remains blocked)")


if __name__ == "__main__":
    main()
