#!/usr/bin/env python3
"""Regression checks for protected-state policy wiring (not ACL qualification)."""
import re
import unittest
from pathlib import Path

SOURCE = Path(__file__).with_name('protect-windows-native-run-state.ps1').read_text(encoding='utf-8')


class ProtectedStateContractTests(unittest.TestCase):
    def test_admin_only_directories_are_verified_with_same_policy(self):
        for name in ('observerInbox', 'evidenceRoot'):
            self.assertEqual(1, len(re.findall(rf'(?m)^\s*Set-RunAcl \${name} \$false \$false\s*$', SOURCE)))
            self.assertEqual(2, len(re.findall(rf'(?m)^\s*Get-ExpectedAcl \${name} \$false \$false\s*$', SOURCE)))

    def test_account_sid_includes_rid(self):
        match = re.search(r"ValidatePattern\('([^']+)'\)\]\[string\]\$SubjectSid", SOURCE)
        self.assertIsNotNone(match)
        pattern = re.compile(match.group(1))
        self.assertIsNotNone(pattern.fullmatch('S-1-5-21-123-456-789-1001'))
        self.assertIsNone(pattern.fullmatch('S-1-5-21-123-456-789'))

    def test_privileged_identity_is_revalidated(self):
        self.assertIn("throw 'P028_SUBJECT_IDENTITY_MISMATCH'", SOURCE)
        self.assertIn("throw 'P028_CONTROLLER_IDENTITY_MISMATCH'", SOURCE)

    def test_unverified_native_advancement_is_blocked(self):
        self.assertIn("throw 'P028_NATIVE_AUTHORITY_UNAVAILABLE'", SOURCE)
        self.assertLess(SOURCE.index("throw 'P028_NATIVE_AUTHORITY_UNAVAILABLE'"),
                        SOURCE.index('$raw = [P028SafeEvidenceReader]::Read'))


    def test_persisted_state_and_prior_evidence_use_bounded_handle_reads(self):
        self.assertNotIn('[IO.File]::ReadAllBytes(', SOURCE)
        for expression in (
            '[P028SafeEvidenceReader]::Read($statePath, 65536)',
            '[P028SafeEvidenceReader]::Read($guiEvidencePath, 4194304)',
            '[P028SafeEvidenceReader]::Read($newEvidencePath, 4194304)',
            '[P028SafeEvidenceReader]::Read($destination, 4194304)',
        ):
            self.assertIn(expression, SOURCE)
        self.assertIn('[void](Assert-NoReparsePath $statePath)', SOURCE)
        self.assertNotIn('Get-FileHash -LiteralPath $destination', SOURCE)

    def test_verify_rejects_unauthorized_pending_native_phase_state(self):
        self.assertIn("throw 'P028_UNAUTHORIZED_STATE_ADVANCEMENT'", SOURCE)
        for condition in (
            "$state.mode -cne 'WINDOWS_PROTECTED_PENDING_NATIVE_AUTHORITY'",
            "$state.status -cne 'ACTIVE'",
            "-not ($state.phases -is [array])",
            "$state.phases.Count -ne 0",
            "$null -ne $state.finalization_digest",
        ):
            self.assertIn(condition, SOURCE)
        self.assertLess(SOURCE.index("throw 'P028_UNAUTHORIZED_STATE_ADVANCEMENT'"),
                        SOURCE.index("if ($Action -eq 'Verify')"))
        self.assertIn("throw 'P028_NATIVE_AUTHORITY_UNAVAILABLE'", SOURCE)

    def test_native_observer_queries_os_sources_under_verified_token(self):
        self.assertIn("$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)", SOURCE)
        self.assertIn("[P028TokenElevationCheck]::IsElevated()", SOURCE)
        self.assertIn("Get-WinEvent -FilterHashtable @{LogName='Security';Id=$EventId", SOURCE)
        self.assertIn("Microsoft-Windows-Security-Auditing", SOURCE)
        self.assertIn("[P028ObserverNative]::ProcessToken", SOURCE)
        self.assertIn("[P028ObserverNative]::WtsSessionIsActive", SOURCE)
        self.assertIn("P028_SUBJECT_TOKEN_NOT_ORDINARY_USER", SOURCE)
        self.assertIn("P028_MACHINE_BOOT_IDENTITY_MISMATCH", SOURCE)
        self.assertIn("boot_time_sha256=(Hash-P028Text", SOURCE)
        self.assertIn("Get-P028Event $item 4624 'Security'", SOURCE)
        self.assertIn("Get-P028Event $item 4634 'Security'", SOURCE)

    def test_observer_records_are_context_bound_and_not_phase_success(self):
        self.assertIn("mode='WINDOWS_NATIVE_OBSERVER'; result='OBSERVED'", SOURCE)
        self.assertIn("machine_guid_sha256=$RunState.machine_guid_sha256", SOURCE)
        self.assertIn("observer_sid_sha256=(Hash-P028Text $identity.User.Value)", SOURCE)
        self.assertIn("controller_sid_sha256=$RunState.controller_sid_sha256", SOURCE)
        self.assertIn("Save-P028ObserverRecord $record $initialPath", SOURCE)
        self.assertIn("Save-P028ObserverRecord $record $logoffPath", SOURCE)
        self.assertIn("Save-P028ObserverRecord $record 'P028-NEW-LOGON-v1.json'", SOURCE)
        self.assertIn("P028_BLOCKED_LOGON_TRIGGER_PROVENANCE_UNAVAILABLE", SOURCE)
        self.assertIn("P028_OBSERVER_RECORD_ALREADY_EXISTS", SOURCE)
        self.assertIn("P028_BLOCKED_OBSERVER_SEQUENCE_LOGON_TRIGGER_UNSUPPORTED", SOURCE)
        self.assertIn("P028_OBSERVER_SEQUENCE_REPLAY_OR_CHAIN_INVALID", SOURCE)
        self.assertIn("P028_OBSERVER_SEQUENCE_SESSION_MISMATCH", SOURCE)

    def test_observer_does_not_open_the_native_phase_gate(self):
        native_guard = SOURCE.index("throw 'P028_NATIVE_AUTHORITY_UNAVAILABLE'")
        phase_read = SOURCE.index("$raw = [P028SafeEvidenceReader]::Read($sourcePath, 4194304)")
        self.assertLess(native_guard, phase_read)
        self.assertLess(SOURCE.index("result='OBSERVED'"), native_guard)

    def test_observer_output_write_is_bounded_atomic_and_single_use(self):
        start = SOURCE.index('function Save-P028ObserverRecord')
        end = SOURCE.index('function Get-P028SecurityEvents', start)
        writer = SOURCE[start:end]
        self.assertIn('[IO.FileMode]::CreateNew', writer)
        self.assertIn('$stream.Flush($true)', writer)
        self.assertIn('[IO.File]::Move($temporary, $destination)', writer)
        self.assertIn('P028_OBSERVER_RECORD_ALREADY_EXISTS', writer)
        self.assertIn('P028SafeEvidenceReader]::Read($destination, 262144)', writer)


if __name__ == '__main__':
    unittest.main()
