"""Regression cases for the P013 Windows evidence gate."""

from __future__ import annotations

import importlib.util
import hashlib
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

MODULE = Path(__file__).with_name("validate-windows-native-evidence.py")
PROBE = Path(__file__).resolve().parents[1] / "crates/client/examples/p013_windows_control_probe.rs"
spec = importlib.util.spec_from_file_location("windows_native_evidence", MODULE)
assert spec and spec.loader
contract = importlib.util.module_from_spec(spec)
spec.loader.exec_module(contract)
SUMMARY_MODULE = Path(__file__).with_name("summarize-windows-native-acceptance.py")
summary_spec = importlib.util.spec_from_file_location("windows_native_summary", SUMMARY_MODULE)
assert summary_spec and summary_spec.loader
summary = importlib.util.module_from_spec(summary_spec)
summary_spec.loader.exec_module(summary)


def passing_record() -> dict:
    checks = {name: "PASS" for name in contract.REQUIRED_CHECKS}
    return {
        "schema_version": 1,
        "result": "PASS",
        "execution_id": "p013-test-run-000001",
        "executed_at_utc": "2026-10-28T00:00:00Z",
        "source_commit": "a" * 40,
        "harness_source_commit": "f" * 40,
        "candidate": {
            "filename": "SynveilSetup.exe", "sha256": "b" * 64, "size_bytes": 123,
            "source_commit": "a" * 40, "release_manifest_sha256": "c" * 64,
            "expected_sha256": "b" * 64, "expected_size_bytes": 123,
            "expected_release_manifest_sha256": "c" * 64,
            "signature_status": "UNSIGNED_FIXTURE",
        },
        "environment": {
            "edition": "Windows 11 Enterprise", "build": 26100, "architecture": "x86_64",
            "harness_revision_sha256": "d" * 64, "qualification": "qualified_isolated_native",
            "operator_approved": True,
        },
        "privilege": {"category": "standard_user", "elevated": False},
        "session": {
            "classification": "genuine_interactive", "interactive_status": "PASS",
            "new_logon_status": "PASS", "trigger_status": "PASS",
            "native_observations": {"input_desktop_open": True, "wts_session_active": True,
                                     "active_console_session_match": True, "wts_session_has_user": True,
                                     "process_user_matches_wts_identity": True},
            "logon_observations": {"security_4624_matches_current_logon_luid": True,
                                   "logon_type_interactive": True, "task_logon_trigger_event": True},
        },
        "checks": checks,
        "cleanup": {"status": "PASS", "independently_verified": True},
        "redaction": {"no_credentials_or_user_paths": True, "raw_pipe_payloads_recorded": False},
        "ui_evidence": {"mode": "native_ui_automation", "state_assertions": True,
                        "screenshots_status": "NOT_CAPTURED"},
        "component_evidence_sha256": {name: "e" * 64 for name in ("environment", "gui", "logon", "logoff", "uninstall", "cleanup")},
    }


class EvidenceContractTests(unittest.TestCase):
    def assertRejected(self, record: dict, fragment: str = "") -> None:
        with self.assertRaises(contract.EvidenceError) as raised:
            contract.validate(record)
        if fragment:
            self.assertIn(fragment, str(raised.exception))

    def test_complete_qualified_evidence_passes(self) -> None:
        contract.validate(passing_record())

    def test_component_evidence_digests_are_required_and_validated(self) -> None:
        record = passing_record()
        del record["component_evidence_sha256"]["gui"]
        self.assertRejected(record, "component_evidence_sha256 is incomplete")
        record = passing_record()
        record["component_evidence_sha256"]["gui"] = "bad"
        self.assertRejected(record, "SHA-256")

    def test_missing_component_records_cannot_aggregate_to_pass(self) -> None:
        candidate = {
            "filename": "SynveilSetup.exe", "sha256": "b" * 64, "size_bytes": 123,
            "source_commit": "a" * 40, "release_manifest_sha256": "c" * 64,
            "expected_sha256": "b" * 64, "expected_size_bytes": 123,
            "expected_release_manifest_sha256": "c" * 64,
            "signature_status": "UNSIGNED_FIXTURE",
        }
        digests = {name: None for name in ("environment", "gui", "logon", "logoff", "uninstall", "cleanup")}
        source_check = summary.current_source_matches
        summary.current_source_matches = lambda _source: True
        try:
            record = summary.make_summary({}, digests, candidate, True, "a" * 40, "f" * 40,
                                          contract.REQUIRED_CHECKS)
        finally:
            summary.current_source_matches = source_check
        self.assertEqual(record["result"], "INCOMPLETE")
        self.assertEqual(record["checks"]["pipe_ping"], "NOT_RUN")
        self.assertEqual(record["checks"]["harness_revision"], "NOT_RUN")
        contract.validate(record)

    def test_component_harness_revision_mismatch_fails_summary(self) -> None:
        candidate = {
            "filename": "SynveilSetup.exe", "sha256": "b" * 64, "size_bytes": 123,
            "source_commit": "a" * 40, "release_manifest_sha256": "c" * 64,
            "expected_sha256": "b" * 64, "expected_size_bytes": 123,
            "expected_release_manifest_sha256": "c" * 64,
            "signature_status": "UNSIGNED_FIXTURE",
        }
        names = ("environment", "gui", "logon", "logoff", "uninstall", "cleanup")
        components = {name: {"result": "PASS", "harness_revision_sha256": "0" * 64} for name in names}
        digests = {name: "e" * 64 for name in names}
        source_check = summary.current_source_matches
        summary.current_source_matches = lambda _source: True
        try:
            record = summary.make_summary(components, digests, candidate, True, "a" * 40,
                                          "f" * 40, contract.REQUIRED_CHECKS)
        finally:
            summary.current_source_matches = source_check
        self.assertEqual(record["checks"]["harness_revision"], "FAIL")
        self.assertEqual(record["result"], "FAIL")
        contract.validate(record)

    def test_summary_rejects_components_from_a_different_harness_commit(self) -> None:
        names = tuple(summary.COMPONENT_FILES)
        components = {
            name: {
                "result": "PASS",
                "harness_revision_sha256": summary.component_harness_digest(name),
            }
            for name in names
        }
        components["gui"]["harness_source_commit"] = "e" * 40
        components["uninstall"]["harness_source_commit"] = "e" * 40
        digests = {name: "e" * 64 for name in names}
        source_check = summary.current_source_matches
        summary.current_source_matches = lambda _source: True
        try:
            record = summary.make_summary(
                components, digests, passing_record()["candidate"], True,
                "a" * 40, "f" * 40, contract.REQUIRED_CHECKS,
            )
        finally:
            summary.current_source_matches = source_check
        self.assertEqual(record["checks"]["harness_revision"], "FAIL")
        self.assertEqual(record["result"], "FAIL")

    def test_summary_cli_records_missing_component_evidence_without_pass(self) -> None:
        source = "a" * 40
        setup_bytes = b"P013 candidate identity fixture"
        setup_hash = hashlib.sha256(setup_bytes).hexdigest()
        with tempfile.TemporaryDirectory(prefix="p013-summary-test-") as temporary:
            root = Path(temporary)
            setup_path = root / "SynveilSetup.exe"
            setup_path.write_bytes(setup_bytes)
            release = {
                "schema_version": 1, "product": "Synveil", "source_commit": source,
                "artifacts": [{"id": "windows-x86_64-installer", "filename": "SynveilSetup.exe",
                               "sha256": setup_hash, "size_bytes": len(setup_bytes)}],
            }
            release_path = root / "release.json"
            release_path.write_text(json.dumps(release), encoding="utf-8")
            manifest_hash = hashlib.sha256(release_path.read_bytes()).hexdigest()
            identity_path = root / "candidate.json"
            identity_path.write_text(json.dumps({
                "filename": "SynveilSetup.exe", "source_commit": source,
                "sha256": setup_hash, "size_bytes": len(setup_bytes),
                "release_manifest_sha256": manifest_hash, "signature_status": "UNSIGNED",
            }), encoding="utf-8")
            evidence_path = root / "components"
            evidence_path.mkdir()
            output_path = root / "summary.json"
            run = subprocess.run([
                sys.executable, str(SUMMARY_MODULE),
                "--candidate-identity", str(identity_path), "--setup", str(setup_path),
                "--release-manifest", str(release_path), "--source-commit", source,
                "--harness-commit", "f" * 40,
                "--evidence-directory", str(evidence_path), "--output", str(output_path),
            ], check=False, capture_output=True, text=True)
            self.assertEqual(run.returncode, 1, run.stderr)
            record = json.loads(output_path.read_text(encoding="utf-8"))
            self.assertEqual(record["result"], "INCOMPLETE")
            self.assertEqual(record["checks"]["candidate_identity"], "PASS")
            self.assertEqual(record["checks"]["component_results"], "NOT_RUN")
            contract.validate(record)

    def test_candidate_digest_mismatch_shape_is_rejected(self) -> None:
        record = passing_record()
        record["candidate"]["sha256"] = "NOT-A-DIGEST"
        self.assertRejected(record, "SHA-256")

    def test_candidate_source_mismatch_is_rejected(self) -> None:
        record = passing_record()
        record["candidate"]["source_commit"] = "e" * 40
        self.assertRejected(record, "does not match")

    def test_harness_revision_commit_is_bound_separately_from_candidate_source(self) -> None:
        record = passing_record()
        self.assertNotEqual(record["source_commit"], record["harness_source_commit"])
        contract.validate(record)
        record["harness_source_commit"] = "bad"
        self.assertRejected(record, "harness_source_commit must be a full commit SHA")

    def test_candidate_digest_must_match_expected_identity(self) -> None:
        record = passing_record()
        record["candidate"]["expected_sha256"] = "e" * 64
        self.assertRejected(record, "does not bind expected")

    def test_candidate_size_must_match_expected_identity(self) -> None:
        record = passing_record()
        record["candidate"]["expected_size_bytes"] = 124
        self.assertRejected(record, "does not bind expected")

    def test_wrong_architecture_is_rejected(self) -> None:
        record = passing_record()
        record["environment"]["architecture"] = "arm64"
        self.assertRejected(record, "x86-64")

    def test_wrong_architecture_can_be_recorded_as_failure(self) -> None:
        record = passing_record()
        record["result"] = "FAIL"
        record["environment"]["architecture"] = "arm64"
        record["checks"]["architecture"] = "FAIL"
        contract.validate(record)

    def test_wrong_windows_build_fails_closed(self) -> None:
        record = passing_record()
        record["environment"]["edition"] = "Windows 10 Enterprise"
        record["environment"]["build"] = 19045
        record["checks"]["windows_build_policy"] = "FAIL"
        self.assertRejected(record, "failed checks")

    def test_standard_user_cannot_claim_elevated_token(self) -> None:
        record = passing_record()
        record["privilege"]["elevated"] = True
        self.assertRejected(record, "elevated token")

    def test_unmarked_daily_use_profile_cannot_claim_disposable_test_identity(self) -> None:
        record = passing_record()
        record["checks"]["disposable_test_user"] = "NOT_RUN"
        self.assertRejected(record, "failed checks")

    def test_presence_without_native_session_observations_is_rejected(self) -> None:
        record = passing_record()
        record["session"]["native_observations"]["input_desktop_open"] = False
        self.assertRejected(record, "independent native observations")

    def test_alternate_credentials_are_not_interactive_evidence(self) -> None:
        record = passing_record()
        record["session"]["classification"] = "alternate_credentials_noninteractive"
        self.assertRejected(record, "cannot prove interaction")

    def test_process_identity_must_match_active_wts_user(self) -> None:
        record = passing_record()
        record["session"]["native_observations"]["process_user_matches_wts_identity"] = False
        self.assertRejected(record, "independent native observations")

    def test_active_wts_desktop_does_not_depend_on_console_session_id(self) -> None:
        record = passing_record()
        record["session"]["native_observations"]["active_console_session_match"] = False
        contract.validate(record)

    def test_new_logon_requires_matching_security_event(self) -> None:
        record = passing_record()
        record["session"]["logon_observations"]["security_4624_matches_current_logon_luid"] = False
        self.assertRejected(record, "matching interactive 4624")

    def test_manual_task_run_is_not_logon_trigger_evidence(self) -> None:
        record = passing_record()
        record["session"]["logon_observations"]["task_logon_trigger_event"] = False
        self.assertRejected(record, "scheduler trigger evidence")

    def test_hosted_windows_cannot_report_pass(self) -> None:
        record = passing_record()
        record["environment"]["qualification"] = "github_hosted_service"
        self.assertRejected(record, "cannot be PASS")

    def test_native_pass_requires_explicit_operator_approval(self) -> None:
        record = passing_record()
        record["environment"]["operator_approved"] = False
        self.assertRejected(record, "operator approval")

    def test_missing_named_pipe_ping_is_not_pass(self) -> None:
        record = passing_record()
        record["checks"]["pipe_ping"] = "NOT_RUN"
        self.assertRejected(record, "failed checks")

    def test_cross_user_authorization_must_pass(self) -> None:
        record = passing_record()
        record["checks"]["pipe_wrong_user"] = "FAIL"
        self.assertRejected(record, "failed checks")

    def test_no_tcp_fallback_must_pass(self) -> None:
        record = passing_record()
        record["checks"]["no_tcp_fallback"] = "FAIL"
        self.assertRejected(record, "failed checks")

    def test_silent_setup_cannot_substitute_for_gui(self) -> None:
        record = passing_record()
        record["checks"]["setup_gui_launch"] = "NOT_RUN"
        self.assertRejected(record, "failed checks")

    def test_nonstandard_install_root_must_fail(self) -> None:
        record = passing_record()
        record["checks"]["install_root"] = "FAIL"
        self.assertRejected(record, "failed checks")

    def test_uninstall_must_preserve_state(self) -> None:
        record = passing_record()
        record["checks"]["uninstall_preservation"] = "FAIL"
        self.assertRejected(record, "failed checks")

    def test_cleanup_requires_independent_verification(self) -> None:
        record = passing_record()
        record["cleanup"]["independently_verified"] = False
        self.assertRejected(record, "independently verified")

    def test_timeout_and_cancellation_are_required(self) -> None:
        record = passing_record()
        record["checks"]["timeout_cancellation"] = "NOT_RUN"
        self.assertRejected(record, "failed checks")

    def test_raw_pipe_payloads_are_forbidden(self) -> None:
        record = passing_record()
        record["redaction"]["raw_pipe_payloads_recorded"] = True
        self.assertRejected(record, "raw pipe messages")

    def test_credentials_and_user_paths_are_forbidden(self) -> None:
        record = passing_record()
        record["username"] = "test-user"
        self.assertRejected(record, "unknown fields")

    def test_failed_cleanup_is_a_failure_not_a_blocker(self) -> None:
        record = passing_record()
        record["result"] = "FAIL"
        record["cleanup"] = {"status": "FAIL", "independently_verified": False}
        contract.validate(record)

    def test_failure_summary_requires_a_failed_observation(self) -> None:
        record = passing_record()
        record["result"] = "FAIL"
        self.assertRejected(record, "no failed scenario")

    def test_wizard_execution_must_remain_disabled(self) -> None:
        record = passing_record()
        record["checks"]["no_wizard_execution_activation"] = "FAIL"
        self.assertRejected(record, "failed checks")

    def test_missing_check_is_incomplete(self) -> None:
        record = passing_record()
        del record["checks"]["real_logon"]
        self.assertRejected(record, "checks are incomplete")

    def test_missing_screenshots_are_classified_when_native_ui_assertions_exist(self) -> None:
        record = passing_record()
        record["ui_evidence"]["screenshots_status"] = "NOT_CAPTURED"
        contract.validate(record)

    def test_missing_ui_assertions_cannot_be_replaced_by_screenshots(self) -> None:
        record = passing_record()
        record["ui_evidence"] = {"mode": "unavailable", "state_assertions": False,
                                 "screenshots_status": "CAPTURED"}
        self.assertRejected(record, "screenshots cannot replace")

    def test_each_required_surface_is_independently_fail_closed(self) -> None:
        for name in sorted(contract.REQUIRED_CHECKS):
            with self.subTest(name=name):
                record = passing_record()
                record["checks"][name] = "NOT_RUN"
                self.assertRejected(record, "failed checks")

    def test_blocked_is_valid_without_claiming_acceptance(self) -> None:
        record = passing_record()
        record["result"] = "BLOCKED_BY_ENVIRONMENT"
        record["environment"]["qualification"] = "github_hosted_service"
        record["session"]["classification"] = "service"
        record["session"]["interactive_status"] = "BLOCKED_BY_ENVIRONMENT"
        record["session"]["new_logon_status"] = "BLOCKED_BY_ENVIRONMENT"
        record["session"]["trigger_status"] = "BLOCKED_BY_ENVIRONMENT"
        record["checks"]["interactive_session"] = "BLOCKED_BY_ENVIRONMENT"
        record["checks"]["real_logon"] = "BLOCKED_BY_ENVIRONMENT"
        contract.validate(record)

    def test_non_windows_pipe_probe_reports_environment_block(self) -> None:
        source = PROBE.read_text(encoding="utf-8")
        self.assertIn('"BLOCKED_BY_ENVIRONMENT"', source)
        self.assertIn('fn unsupported_platform()', source)
        self.assertIn('"unsupported_platform"', source)

    def test_gui_harness_binds_candidate_and_harness_commits_separately(self) -> None:
        source = (Path(__file__).resolve().parents[1] / "scripts/test-windows-gui-journey.ps1").read_text(encoding="utf-8")
        self.assertIn("[string]$ExpectedSourceCommit", source)
        self.assertIn("[string]$ExpectedHarnessCommit", source)
        self.assertIn("$release.source_commit -ceq $ExpectedSourceCommit", source)
        self.assertIn("Initialize-ControlIpcProbe $RepositoryRoot $ExpectedHead", source)
        self.assertIn("Invoke-ControlIpcProbe $repoRoot $ExpectedHarnessCommit 'owner'", source)

    def test_uninstall_is_separate_and_keeps_candidate_through_logon(self) -> None:
        root = Path(__file__).resolve().parents[1]
        gui = (root / "scripts/test-windows-gui-journey.ps1").read_text(encoding="utf-8")
        uninstall = (root / "scripts/test-windows-installed-uninstall.ps1").read_text(encoding="utf-8")
        aggregator = SUMMARY_MODULE.read_text(encoding="utf-8")
        self.assertNotIn("Get-OwnedRegisteredUninstaller", gui)
        self.assertIn("uninstall_preservation = 'NOT_RUN'", gui)
        self.assertIn("$ExpectedSourceCommit", uninstall)
        self.assertIn("$ExpectedHarnessCommit", uninstall)
        self.assertIn("Assert-P013 ($OperatorApproval -ceq $approvalText)", uninstall)
        self.assertIn("ProcessIsElevated([uint32]$uninstallerProcess.Id)", uninstall)
        self.assertIn("Invoke-PipeProbe 'shutdown'", uninstall)
        self.assertIn('"uninstall": "windows-uninstall-preservation-v1.json"', aggregator)
        self.assertIn('status(uninstall_checks.get("uninstall_preservation"))', aggregator)


if __name__ == "__main__":
    unittest.main()
