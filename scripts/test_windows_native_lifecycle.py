"""Portable adapter-contract tests; no Windows token or event APIs are executed."""
import datetime as dt
import hashlib
import json
import os
import re
import shutil
import sqlite3
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import windows_native_lifecycle as lifecycle
import windows_native_coordinator as fixture_coordinator


def digest(value: str) -> str:
    return hashlib.sha256(value.encode()).hexdigest()


class FakeObserver:
    def observe(self, phase, context):
        return make_observation(phase, context)


class FakeVerifier:
    def verify(self, phase, context, observation):
        return lifecycle.VerificationResult(True, digest('independent-verifier'), len(observation.references), 'VERIFIED')


class FailingVerifier:
    def verify(self, phase, context, observation):
        return lifecycle.VerificationResult(False, digest('independent-verifier'), 0, 'API_UNAVAILABLE')


def make_context(**changes):
    now = dt.datetime.now(dt.timezone.utc).replace(microsecond=0)
    values = dict(
        run_id='1' * 32,
        machine_sha256=digest('machine'),
        boot_sha256=digest('boot'),
        source_commit='a' * 40,
        harness_commit='b' * 40,
        candidate_sha256=digest('candidate'),
        manifest_sha256=digest('manifest'),
        controller_sid_sha256=digest('controller'),
        subject_sid_sha256=digest('subject'),
        observer_sid_sha256=digest('observer'),
        created_at_utc=now.isoformat(),
        expires_at_utc=(now + dt.timedelta(hours=1)).isoformat(),
    )
    values.update(changes)
    return lifecycle.RunContext(**values)


def make_observation(phase, context):
    created = dt.datetime.fromisoformat(context.created_at_utc)
    if phase == 'LOGON_TRIGGER_VERIFIED':
        old_luid, new_luid = digest('initial-authentication-luid'), digest('subsequent-authentication-luid')
        instance = digest('task-instance')
        process_time = 133000000000000000
        records = [
            ('Security', 4624, 10, 1, old_luid, 2, 1, '', 0),
            ('Security', 4634, 11, 5, old_luid, 0, 1, '', 0),
            ('Security', 4624, 12, 10, new_luid, 2, 2, '', 0),
            ('TaskScheduler', 119, 20, 11, '', 0, 0, instance, 0),
            ('TaskScheduler', 100, 21, 12, '', 0, 0, instance, 0),
            ('TaskScheduler', 129, 22, 13, new_luid, 0, 2, instance, 1234),
            ('TaskScheduler', 200, 23, 14, '', 0, 0, instance, 0),
            ('Security', 4688, 13, 13, new_luid, 0, 2, '', 1234),
        ]
        refs = tuple(lifecycle.EvidenceReference(
            source=source, event_id=event_id, record_id=record_id,
            record_sha256=digest(f'{source}-{event_id}-{record_id}'),
            occurred_at_utc=(created + dt.timedelta(seconds=seconds)).isoformat(),
            machine_sha256=context.machine_sha256, boot_sha256=context.boot_sha256,
            run_id=context.run_id, task_instance_sha256=instance_id,
            process_id=process_id, process_created_filetime=process_time if event_id in {129, 4688} else 0,
            subject_sid_sha256=context.subject_sid_sha256 if source == 'Security' else '',
            session_id=session_id, authentication_luid_sha256=luid, logon_type=logon_type,
        ) for source, event_id, record_id, seconds, luid, logon_type, session_id, instance_id, process_id in records)
        return lifecycle.Observation(phase, hashlib.sha256(context.canonical()).hexdigest(),
                                     refs, context.observer_sid_sha256)
    ids = {
        'PREFLIGHT': [('Security', 4624)],
        'LOGOFF_OBSERVED': [('Security', 4634)],
        'NEW_LOGON_OBSERVED': [('Security', 4624)],
    }.get(phase, [('Security', 4624)])
    refs = tuple(lifecycle.EvidenceReference(
        source=source,
        event_id=event_id,
        record_id=index + 10,
        record_sha256=digest(f'{source}-{event_id}-{index}'),
        occurred_at_utc=(created + dt.timedelta(seconds=index + 1)).isoformat(),
        machine_sha256=context.machine_sha256,
        boot_sha256=context.boot_sha256,
        run_id=context.run_id,
        task_instance_sha256=digest('instance') if source == 'TaskScheduler' else '',
        process_id=1234 if event_id in {129, 4688} else 0,
        process_created_filetime=133000000000000000 if event_id in {129, 4688} else 0,
        subject_sid_sha256=context.subject_sid_sha256 if source == 'Security' else '',
        session_id=5 if source == 'Security' and event_id in {4624, 4634} else 0,
        authentication_luid_sha256=digest('fixture-auth-luid') if source == 'Security' and event_id in {4624, 4634} else '',
        logon_type=2 if event_id == 4624 else 0,
    ) for index, (source, event_id) in enumerate(ids))
    return lifecycle.Observation(phase, hashlib.sha256(context.canonical()).hexdigest(),
                                 refs, context.observer_sid_sha256)


class LifecycleContractTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.path = Path(self.tmp.name) / lifecycle.NATIVE_JOURNAL_NAME
        self.context = make_context()
        self.journal = lifecycle.NativePendingJournal.initialize(
            self.path, self.context, approval='I_APPROVE_P028_NATIVE_PENDING_JOURNAL')

    def rejected(self, reason, callback, *args, **kwargs):
        with self.assertRaises(lifecycle.LifecycleError) as error:
            callback(*args, **kwargs)
        self.assertEqual(str(error.exception), reason)

    def test_plan_is_complete_but_never_reports_execution(self):
        for mode in ('FIXTURE_ONLY', 'WINDOWS_NATIVE'):
            plan = lifecycle.orchestration_plan(mode)
            self.assertEqual([step['phase'] for step in plan['steps']], list(lifecycle.PHASES))
            self.assertTrue(all(step['state'] == 'NOT_RUN' for step in plan['steps']))
            self.assertTrue(plan['automatic_replay'] is False)
            self.assertEqual(plan['native_authority'], 'BLOCKED')
        self.assertTrue(next(x for x in plan['steps'] if x['phase'] == 'INSTALLED')['explicit_approval_required'])

    def test_fixture_plan_exercises_order_without_authorizing_native_advance(self):
        passed = lifecycle.simulate_fixture_plan({phase: 'PASS' for phase in lifecycle.PHASES})
        self.assertEqual(passed['result'], 'FIXTURE_ONLY_COMPLETE')
        self.assertEqual(passed['native_result'], 'P028_NATIVE_AUTHORITY_UNAVAILABLE')
        self.assertFalse(passed['host_mutations_performed'])
        self.assertTrue(all(step['evidence'] == 'SYNTHETIC_FIXTURE' for step in passed['steps']))

        blocked = lifecycle.simulate_fixture_plan({'PREFLIGHT': 'PASS', 'CANDIDATE_VERIFIED': 'PASS',
            'ENVIRONMENT_QUALIFIED': 'PASS', 'INSTALLED': 'PASS', 'GUI_VERIFIED': 'PASS',
            'IPC_VERIFIED': 'PASS', 'STARTUP_CONFIGURED': 'PASS', 'LOGOFF_OBSERVED': 'PASS',
            'NEW_LOGON_OBSERVED': 'PASS', 'LOGON_TRIGGER_VERIFIED': 'BLOCKED'})
        self.assertEqual(blocked['result'], 'FIXTURE_ONLY_INCOMPLETE')
        self.assertEqual(blocked['steps'][10]['status'], 'NOT_RUN')
        self.assertEqual(blocked['native_authority'], 'BLOCKED')

        interrupted = lifecycle.simulate_fixture_plan({'PREFLIGHT': 'PASS'}, interrupted_at='CANDIDATE_VERIFIED')
        self.assertEqual(interrupted['result'], 'FIXTURE_ONLY_INTERRUPTED')
        self.assertEqual(interrupted['steps'][1]['status'], 'INTERRUPTED')
        with self.assertRaises(lifecycle.LifecycleError) as error:
            lifecycle.simulate_fixture_plan({'PREFLIGHT': 'FAIL', 'CANDIDATE_VERIFIED': 'PASS'})
        self.assertEqual(str(error.exception), 'FIXTURE_OUTCOME_AFTER_HALT')

    def test_source_contracts_keep_native_authority_and_role_harness_guarded(self):
        root = Path(__file__).resolve().parent.parent
        role = (root / 'scripts/qualify-windows-native-role-access.ps1').read_text()
        probe = (root / 'scripts/windows-native-role-probe.ps1').read_text()
        task = (root / 'scripts/windows-native-logon-task.ps1').read_text()
        protected = (root / 'scripts/protect-windows-native-run-state.ps1').read_text()
        workflow = (root / '.github/workflows/windows-native-acceptance.yml').read_text()
        self.assertIn('I_APPROVE_DISPOSABLE_SYNVEIL_NATIVE_ROLE_ACCESS', role)
        self.assertIn("$env:GITHUB_ACTIONS -eq 'true'", role)
        self.assertIn("$env:PROCESSOR_ARCHITECTURE -ceq 'AMD64'", role)
        self.assertIn('Start-P028RoleProbe', role)
        self.assertIn('MaximumAccess', role)
        self.assertIn('CandidateSha256', role)
        self.assertIn('ManifestSha256', role)
        self.assertIn('ExpectedSourceCommit', role)
        self.assertIn('$manifestReadSha -cne $manifestInfo.Sha256', role)
        self.assertIn('HarnessComponents=$components', role)
        self.assertIn("native_acceptance = 'BLOCKED_NOT_RUN'", role)
        self.assertIn('P028_NATIVE_AUTHORITY_UNAVAILABLE', protected)
        self.assertIn("$P028SchedulerProofIds = @(119,100,129,200)", task)
        self.assertIn("107,108,109,110", task)
        self.assertIn('client_file_identity_sha256', task)
        self.assertIn('BLOCKED_MAPPED_IMAGE_PROVENANCE_UNAVAILABLE', task)
        self.assertIn('Assert-WriteDenied', probe)
        self.assertIn('Assert-CreateDenied', probe)
        self.assertIn('P028_ROLE_WRITE_WAS_ALLOWED', probe)
        self.assertIn("Get-WinEvent -LogName 'Security' -MaxEvents 1", probe)
        self.assertIn("Get-WinEvent -LogName 'Microsoft-Windows-TaskScheduler/Operational' -MaxEvents 1", probe)
        self.assertIn('P028_ROLE_UNEXPECTED_NESTED_GROUP', (root / 'scripts/windows-native-observer-support.ps1').read_text())
        self.assertIn('P028_CONTROLLER_UNEXPECTED_GROUP', (root / 'scripts/windows-native-observer-support.ps1').read_text())
        self.assertIn('AssertControllerToken([uint32]$PID', role)
        self.assertIn('S-1-5-21-', (root / 'scripts/windows-native-observer-support.ps1').read_text())
        # CI parses the opt-in mutating harness but never invokes it.
        self.assertIn("'scripts/qualify-windows-native-role-access.ps1'", workflow)
        self.assertNotRegex(workflow, r'(?m)^\s*run:.*qualify-windows-native-role-access')
        self.assertNotRegex(workflow, r'(?m)^\s*run:.*windows-native-role-probe')

    def test_native_journal_requires_approval_and_separate_path(self):
        second = self.path.parent / lifecycle.NATIVE_JOURNAL_NAME
        self.rejected('NATIVE_JOURNAL_ALREADY_EXISTS', lifecycle.NativePendingJournal.initialize,
                      second, self.context, approval='I_APPROVE_P028_NATIVE_PENDING_JOURNAL')
        self.rejected('NATIVE_JOURNAL_APPROVAL_REQUIRED', lifecycle.NativePendingJournal.initialize,
                      self.path.parent / 'other.sqlite3', self.context, approval='yes')
        self.rejected('NATIVE_JOURNAL_PATH_UNSAFE', lifecycle.NativePendingJournal.initialize,
                      self.path.parent / 'fixture.sqlite3', self.context,
                      approval='I_APPROVE_P028_NATIVE_PENDING_JOURNAL')
        self.rejected('NATIVE_JOURNAL_PATH_UNSAFE', lifecycle.NativePendingJournal.initialize,
                      Path(lifecycle.NATIVE_JOURNAL_NAME), self.context,
                      approval='I_APPROVE_P028_NATIVE_PENDING_JOURNAL')
        self.assertEqual(self.journal.status()['native_result'], 'P028_NATIVE_AUTHORITY_UNAVAILABLE')

    def test_fixture_database_cannot_be_opened_as_native_journal(self):
        fixture_dir = self.path.parent / 'fixture'
        fixture_dir.mkdir()
        fixture_path = fixture_dir / 'p028-fixture-state.sqlite3'
        expiry = (dt.datetime.now(dt.timezone.utc) + dt.timedelta(hours=1)).isoformat()
        fixture_coordinator.initialize(fixture_path, fixture=True,
            approval='I_APPROVE_DISPOSABLE_FIXTURE_ONLY', expires=expiry)
        copied_native_name = self.path.parent / 'fixture-copy' / lifecycle.NATIVE_JOURNAL_NAME
        copied_native_name.parent.mkdir()
        shutil.copyfile(fixture_path, copied_native_name)
        self.rejected('NATIVE_JOURNAL_CORRUPT', lifecycle.NativePendingJournal(copied_native_name).status)

    def test_context_rejects_role_collision_and_cross_context_replay(self):
        self.rejected('ROLE_IDENTITY_COLLISION', make_context(
            observer_sid_sha256=digest('controller')).validate)
        observation = make_observation('PREFLIGHT', self.context)
        other = make_context(run_id='2' * 32)
        self.rejected('OBSERVATION_CONTEXT_MISMATCH', observation.validate, other)

    def test_observation_requires_exact_supported_scheduler_and_security_records(self):
        good = make_observation('LOGON_TRIGGER_VERIFIED', self.context)
        good.validate(self.context)
        one_missing = lifecycle.Observation(
            good.phase, good.context_sha256, good.references[:-1], good.observer_identity_sha256)
        self.rejected('EVIDENCE_REFERENCE_SET_INCOMPLETE', one_missing.validate, self.context)
        unsupported = lifecycle.EvidenceReference(
            'TaskScheduler', 107, 99, digest('event'), self.context.created_at_utc,
            self.context.machine_sha256, self.context.boot_sha256, self.context.run_id,
            task_instance_sha256=digest('instance'))
        self.rejected('EVENT_REFERENCE_INVALID', unsupported.validate, self.context, 'LOGON_TRIGGER_VERIFIED')

    def test_logon_trigger_rejects_session_luid_subject_and_instance_mismatches(self):
        good = make_observation('LOGON_TRIGGER_VERIFIED', self.context)
        good.validate(self.context)
        mutations = (
            ('TASK_INSTANCE_CONTEXT_MISMATCH', 6, {'task_instance_sha256': digest('other-instance')}),
            ('SUBJECT_IDENTITY_MISMATCH', 7, {'subject_sid_sha256': digest('other-subject')}),
            ('TASK_PROCESS_SESSION_MISMATCH', 5, {'session_id': 999}),
            ('TASK_PROCESS_SESSION_MISMATCH', 5, {'authentication_luid_sha256': digest('other-luid')}),
            ('LOGOFF_NEW_LOGON_SEQUENCE_INVALID', 1, {'authentication_luid_sha256': digest('other-luid')}),
            ('TASK_EVENT_ORDER_INVALID', 3, {'occurred_at_utc': good.references[2].occurred_at_utc}),
        )
        for reason, index, changes in mutations:
            with self.subTest(reason=reason, changes=changes):
                references = list(good.references)
                references[index] = lifecycle.EvidenceReference(**{**references[index].__dict__, **changes})
                observation = lifecycle.Observation(good.phase, good.context_sha256,
                    tuple(references), good.observer_identity_sha256)
                self.rejected(reason, observation.validate, self.context)
        references = list(good.references)
        references.append(lifecycle.EvidenceReference(**{**references[3].__dict__, 'record_id': 99}))
        ambiguous = lifecycle.Observation(good.phase, good.context_sha256,
            tuple(references), good.observer_identity_sha256)
        self.rejected('TASK_INSTANCE_AMBIGUOUS', ambiguous.validate, self.context)
        references = list(good.references)
        references[1] = lifecycle.EvidenceReference(**{**references[1].__dict__,
            'record_id': references[0].record_id})
        replayed = lifecycle.Observation(good.phase, good.context_sha256,
            tuple(references), good.observer_identity_sha256)
        self.rejected('EVIDENCE_REFERENCE_DUPLICATE', replayed.validate, self.context)

    def test_rejects_duplicate_cross_machine_cross_boot_and_stale_references(self):
        observation = make_observation('PREFLIGHT', self.context)
        first = observation.references[0]
        duplicate = lifecycle.Observation(observation.phase, observation.context_sha256,
                                          (first, first), observation.observer_identity_sha256)
        self.rejected('EVIDENCE_REFERENCE_DUPLICATE', duplicate.validate, self.context)
        wrong_machine = lifecycle.EvidenceReference(**{**first.__dict__, 'machine_sha256': digest('other')})
        cross_machine = lifecycle.Observation(observation.phase, observation.context_sha256,
                                              (wrong_machine,), observation.observer_identity_sha256)
        self.rejected('EVIDENCE_CONTEXT_MISMATCH', cross_machine.validate, self.context)
        wrong_boot = lifecycle.EvidenceReference(**{**first.__dict__, 'boot_sha256': digest('other-boot')})
        cross_boot = lifecycle.Observation(observation.phase, observation.context_sha256,
                                           (wrong_boot,), observation.observer_identity_sha256)
        self.rejected('EVIDENCE_CONTEXT_MISMATCH', cross_boot.validate, self.context)
        stale = lifecycle.EvidenceReference(**{**first.__dict__, 'occurred_at_utc': '2000-01-01T00:00:00+00:00'})
        stale_observation = lifecycle.Observation(observation.phase, observation.context_sha256,
                                                  (stale,), observation.observer_identity_sha256)
        self.rejected('EVIDENCE_TIME_OUT_OF_RUN', stale_observation.validate, self.context)

    def test_observer_subject_and_controller_cannot_be_verifier(self):
        result = lifecycle.VerificationResult(True, self.context.observer_sid_sha256, 1, 'VERIFIED')
        self.rejected('VERIFIER_ROLE_COLLISION', result.validate, self.context)
        result = lifecycle.VerificationResult(True, self.context.controller_sid_sha256, 1, 'VERIFIED')
        self.rejected('VERIFIER_ROLE_COLLISION', result.validate, self.context)
        result = lifecycle.VerificationResult(True, self.context.subject_sid_sha256, 1, 'VERIFIED')
        self.rejected('VERIFIER_ROLE_COLLISION', result.validate, self.context)

    def test_synthetic_observation_and_verifier_never_unlock_native_advancement(self):
        coordinator = lifecycle.NativeLifecycleCoordinator(self.journal, FakeObserver(), FakeVerifier())
        result = coordinator.observe_next('PREFLIGHT', self.context)
        self.assertEqual(result['status'], 'VERIFIED_PENDING_AUTHORITY')
        self.assertEqual(result['reason'], 'P028_NATIVE_AUTHORITY_UNAVAILABLE')
        state = self.journal.status()
        self.assertEqual(state['completed_phases'], [])
        self.assertEqual(state['pending']['checkpoint_state'], 'VERIFIED_PENDING_AUTHORITY')
        self.rejected('NATIVE_AUTHORITY_UNAVAILABLE', self.journal.begin_observation, 'CANDIDATE_VERIFIED')
        with self.assertRaises(TypeError):
            lifecycle.NativeLifecycleCoordinator(self.journal, FakeObserver(), FakeVerifier(), object())

    def test_independent_verifier_api_failure_is_persisted_as_blocked(self):
        coordinator = lifecycle.NativeLifecycleCoordinator(self.journal, FakeObserver(), FailingVerifier())
        result = coordinator.observe_next('PREFLIGHT', self.context)
        self.assertEqual(result['status'], 'BLOCKED_VERIFICATION_FAILED')
        self.assertEqual(self.journal.status()['pending']['checkpoint_state'], 'BLOCKED_VERIFICATION_FAILED')

    def test_interruption_requires_reconciliation_and_never_replays(self):
        attempt = self.journal.begin_observation('PREFLIGHT')
        recovery = self.journal.recover_interruption()
        self.assertEqual(recovery['recovery'], 'MANUAL_RECONCILIATION_REQUIRED')
        self.assertFalse(recovery['automatic_replay'])
        self.rejected('NATIVE_AUTHORITY_UNAVAILABLE', self.journal.begin_observation, 'PREFLIGHT')
        observation = make_observation('PREFLIGHT', self.context)
        self.journal.record_observation('PREFLIGHT', attempt, observation, self.context)
        self.assertEqual(self.journal.recover_interruption()['checkpoint_state'], 'OBSERVATION_PENDING_VERIFICATION')

    def test_corrupt_checkpoint_and_partial_transaction_fail_closed(self):
        attempt = self.journal.begin_observation('PREFLIGHT')
        observation = make_observation('PREFLIGHT', self.context)
        self.journal.record_observation('PREFLIGHT', attempt, observation, self.context)
        db = sqlite3.connect(self.path)
        try:
            db.execute('UPDATE checkpoint SET observation=?', (b'changed',))
            db.commit()
        finally:
            db.close()
        self.rejected('NATIVE_OBSERVATION_CORRUPT', self.journal.status)

    def test_verification_receipt_digest_and_state_are_rechecked(self):
        coordinator = lifecycle.NativeLifecycleCoordinator(self.journal, FakeObserver(), FakeVerifier())
        coordinator.observe_next('PREFLIGHT', self.context)
        db = sqlite3.connect(self.path)
        try:
            db.execute('UPDATE checkpoint SET verification=?', (b'tampered',))
            db.commit()
        finally:
            db.close()
        self.rejected('NATIVE_VERIFICATION_CORRUPT', self.journal.status)

    def test_interrupted_sql_transaction_rolls_back_checkpoint_write(self):
        db = sqlite3.connect(self.path, isolation_level=None)
        try:
            db.execute('BEGIN IMMEDIATE')
            db.execute('INSERT INTO checkpoint VALUES (0, ?, ?, ?, NULL, NULL, NULL, NULL, ?)',
                       ('PREFLIGHT', 'OBSERVATION_STARTED', 'partial-attempt', 'partial-time'))
        finally:
            db.close()
        self.assertIsNone(self.journal.status()['pending'])
        attempt = self.journal.begin_observation('PREFLIGHT')
        self.assertEqual(len(attempt), 32)

    def test_symlink_journal_path_is_rejected(self):
        link = self.path.parent / 'link.sqlite3'
        try:
            os.symlink(self.path, link)
        except (OSError, NotImplementedError):
            self.skipTest('symlink creation unavailable')
        aliased = lifecycle.NativePendingJournal(link)
        self.rejected('NATIVE_JOURNAL_PATH_UNSAFE', aliased.status)

    def test_symlink_parent_path_is_rejected(self):
        target = self.path.parent / 'protected-target'
        target.mkdir()
        link = self.path.parent / 'parent-link'
        try:
            os.symlink(target, link, target_is_directory=True)
        except (OSError, NotImplementedError):
            self.skipTest('directory symlink creation unavailable')
        aliased = lifecycle.NativePendingJournal(link / lifecycle.NATIVE_JOURNAL_NAME)
        self.rejected('NATIVE_JOURNAL_PATH_UNSAFE', aliased.status)

    def test_write_side_leaf_substitution_during_open_is_rejected(self):
        outside = self.path.parent / 'outside.sqlite3'
        shutil.copyfile(self.path, outside)
        displaced = self.path.parent / 'displaced.sqlite3'
        original_connect = sqlite3.connect

        def substitute_then_open(*args, **kwargs):
            self.path.rename(displaced)
            shutil.copyfile(outside, self.path)
            return original_connect(*args, **kwargs)

        with mock.patch.object(lifecycle.sqlite3, 'connect', side_effect=substitute_then_open):
            self.rejected('NATIVE_JOURNAL_PATH_CHANGED_DURING_OPEN', self.journal.status)
        self.path.unlink(missing_ok=True)
        displaced.rename(self.path)
        self.assertEqual(self.journal.status()['status'], 'ACTIVE_BLOCKED')


if __name__ == '__main__':
    unittest.main()
