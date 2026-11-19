#!/usr/bin/env python3
"""Regression checks for protected-state policy wiring (not ACL qualification)."""
import re
import unittest
from pathlib import Path

SOURCE = Path(__file__).with_name('protect-windows-native-run-state.ps1').read_text(encoding='utf-8')


class ProtectedStateContractTests(unittest.TestCase):
    def test_protected_directories_are_verified_with_same_policy(self):
        for name, role in (('observerInbox', ' Write'), ('evidenceRoot', '')):
            expression = rf'\${name} \$false \$false{role}'
            self.assertEqual(1, len(re.findall(rf'(?m)^\s*Set-RunAcl {expression}\s*$', SOURCE)))
            self.assertEqual(2, len(re.findall(rf'(?m)^\s*Get-ExpectedAcl {expression}\s*$', SOURCE)))

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
        self.assertIn("[P028ObserverAccess]::IsElevated([uint32]$PID, $identity.User.Value)", SOURCE)
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

    def test_sequence_verifier_requeries_exact_os_events(self):
        start = SOURCE.index('function Get-P028SecurityEventByRecordId')
        end = SOURCE.index('function Invoke-P028ObserverAction', start)
        verifier = SOURCE[start:end]
        self.assertIn("Get-WinEvent -LogName 'Security' -FilterXPath $filter -MaxEvents 2", verifier)
        self.assertIn("$EventId -notin @(4624,4634)", verifier)
        self.assertIn("$RecordId -le 0", verifier)
        self.assertIn("$actual.XmlSha256 -cne [string]$Record.event.record_sha256", verifier)
        self.assertIn("$actual.Data.TargetUserSid -cne $ExpectedSid", verifier)
        self.assertIn('(Hash-P028Text $actualLuid) -cne $ExpectedLuidHash', verifier)
        self.assertIn("$actual.Data.LogonType -notin @('2','10')", verifier)

        start = SOURCE.index('function Verify-P028ObserverSequence')
        end = SOURCE.index('$allowedRoot =', start)
        sequence = SOURCE[start:end]
        for record, event_id in (('initial', 4624), ('logoff', 4634), ('newLogon', 4624)):
            self.assertIn(f'Assert-P028ObserverEventRequery ${record}.Record {event_id}', sequence)
        self.assertLess(sequence.index('Assert-P028ObserverEventRequery'),
                        sequence.index('$records = @($initial.Record,$logoff.Record,$newLogon.Record)'))

    def test_os_event_requery_does_not_open_native_authority_gate(self):
        self.assertIn('P028_OBSERVER_EVENT_REQUERY_MISMATCH', SOURCE)
        self.assertIn('P028_BLOCKED_OBSERVER_SEQUENCE_LOGON_TRIGGER_UNSUPPORTED', SOURCE)
        self.assertIn("throw 'P028_NATIVE_AUTHORITY_UNAVAILABLE'", SOURCE)
        self.assertLess(SOURCE.index("throw 'P028_NATIVE_AUTHORITY_UNAVAILABLE'"),
                        SOURCE.index('$raw = [P028SafeEvidenceReader]::Read($sourcePath, 4194304)'))

    def test_controller_gate_does_not_consume_observer_assertions(self):
        role_gate = SOURCE.index("if ($isObserverAction)")
        state_read = SOURCE.index("$stateBytes = [P028SafeEvidenceReader]::Read")
        self.assertLess(role_gate, state_read)
        self.assertIn("$identity.User -eq $observer", SOURCE[role_gate:state_read])
        self.assertIn("if (-not $isObserverAction -and $state.controller_sid_sha256 -cne $controllerHash)", SOURCE)
        self.assertIn("if ($observer -eq $subject)", SOURCE)
        self.assertIn("Assert-P028RoleAccess", SOURCE)

    def test_ancestor_and_leaf_native_reads_are_checked(self):
        reader = Path(__file__).with_name('windows-native-observer-support.ps1').read_text(encoding='utf-8')
        for guard in ("handles.Add(Open(parents[index].FullName, true))",
                      "info.LinkCount != 1", "info.DeletePending != 0",
                      "directory ? 0x02200000u : 0x00200000u", "directory ? 0x81u"):
            self.assertIn(guard, reader)
        self.assertIn("Assert-P028CanonicalPath $ApprovedRoot", SOURCE)

    def test_executable_disk_file_identity_is_handle_backed_but_not_mapped_image_proof(self):
        reader = Path(__file__).with_name('windows-native-observer-support.ps1').read_text(encoding='utf-8')
        task = Path(__file__).with_name('windows-native-logon-task.ps1').read_text(encoding='utf-8')
        self.assertIn('struct FileIdInfo', reader)
        self.assertIn('public ulong FileIdLow, FileIdHigh', reader)
        self.assertIn('GetFileInformationByHandleEx(handle, 18, out id', reader)
        self.assertIn('public static FileInspection Inspect', reader)
        self.assertIn('P028SafeEvidenceReader]::Inspect($taskContext.ClientPath', task)
        self.assertIn('client_file_identity_sha256=$clientInspection.FileIdentitySha256', task)
        self.assertIn("mapped_image_identity='UNAVAILABLE_FROM_SUPPORTED_USER_MODE_API'", task)
        self.assertIn("causality_status='BLOCKED_TASK_EVENT_HAS_NO_AUTHENTICATION_LUID'", task)
        self.assertIn("mapped_image_status='BLOCKED_MAPPED_IMAGE_PROVENANCE_UNAVAILABLE'", task)
        self.assertIn("status='BLOCKED_NATIVE_LOGON_TRIGGER_UNVERIFIED'", task)

    def test_real_role_qualification_rejects_nested_groups_and_reads_both_event_channels(self):
        observer = Path(__file__).with_name('windows-native-observer-support.ps1').read_text(encoding='utf-8')
        probe = Path(__file__).with_name('windows-native-role-probe.ps1').read_text(encoding='utf-8')
        self.assertIn('P028_ROLE_UNEXPECTED_NESTED_GROUP', observer)
        self.assertIn('P028_CONTROLLER_UNEXPECTED_GROUP', observer)
        self.assertIn('sid.StartsWith("S-1-5-21-"', observer)
        self.assertIn("Get-WinEvent -LogName 'Security' -MaxEvents 1", probe)
        self.assertIn("Get-WinEvent -LogName 'Microsoft-Windows-TaskScheduler/Operational' -MaxEvents 1", probe)
        harness = Path(__file__).with_name('qualify-windows-native-role-access.ps1').read_text(encoding='utf-8')
        self.assertIn('CandidateSha256', harness)
        self.assertIn('ManifestSha256', harness)
        self.assertIn('HarnessComponents=$components', harness)

    def test_phase_evidence_is_durable_before_checkpoint_and_retry_is_idempotent(self):
        start = SOURCE.index("$destination = Join-Path $evidenceRoot ($Phase + '.json')")
        end = SOURCE.index('Write-Output "P028_PHASE_RECORDED', start)
        advance = SOURCE[start:end]
        self.assertIn('[IO.FileMode]::CreateNew', advance)
        self.assertIn('$out.Flush($true)', advance)
        self.assertIn('[IO.File]::Move($destTemp, $destination)', advance)
        self.assertIn("if ($existingDigest -cne $digest) { throw 'P028_EVIDENCE_REPLAY' }", advance)
        self.assertLess(advance.index("[IO.File]::Move($destTemp, $destination)"),
                        advance.index('$state.phases += [ordered]@'))
        self.assertIn('Save-RunState $state', advance)

        start = SOURCE.index('function Save-RunState')
        end = SOURCE.index('function Hash-P028Text', start)
        checkpoint = SOURCE[start:end]
        self.assertIn('[IO.FileMode]::CreateNew', checkpoint)
        self.assertIn('$stream.Flush($true)', checkpoint)
        self.assertIn('[IO.File]::Move($temporary, $statePath, $true)', checkpoint)


if __name__ == '__main__':
    unittest.main()
