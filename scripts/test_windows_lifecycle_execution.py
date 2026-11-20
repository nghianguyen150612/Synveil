#!/usr/bin/env python3
"""Executable 13-phase fixture integration and adversarial recovery tests."""
import dataclasses
import hashlib
import json
from pathlib import Path, PurePosixPath, PureWindowsPath
import sqlite3
import tempfile
from concurrent.futures import ThreadPoolExecutor
import unittest
from unittest.mock import patch

import windows_lifecycle_execution as e
import windows_lifecycle_adapters as a
from test_windows_native_lifecycle import make_context


class Runner:
    def __init__(self):
        self.calls = []

    def execute(self, phase, attempt, context):
        self.calls.append((phase, attempt))


class Verifier:
    def requery(self, phase, attempt, context):
        return {'mode': 'FIXTURE_ONLY', 'context': e.digest(dataclasses.asdict(context)),
                'phase': phase, 'attempt': attempt, 'reference': e.digest([phase, attempt]),
                'outcome': 'CONFIRMED_SUCCESS'}


class Inspector:
    def query(self, context, resource):
        return 'RETAINED_PROTECTED' if resource.disposition == 'RETAIN_PROTECTED' else 'ABSENT'


class ExecutionTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.ctx = make_context()
        self.path = Path(self.tmp.name) / 'p028-execution-fixture.sqlite3'
        self.engine = e.FixtureExecution(self.path, self.ctx)
        self.inventory = [e.Resource(k, e.digest(k)) for k in sorted(e.KINDS)]
        self.inventory += [e.Resource('protected_state', e.digest('audit'), 'RETAIN_PROTECTED')]
        self.engine.initialize(self.inventory)
        self.runner, self.verifier, self.inspector = Runner(), Verifier(), Inspector()

    def fails(self, reason, fn, *args, **kwargs):
        with self.assertRaisesRegex(e.LifecycleError, reason):
            fn(*args, **kwargs)

    def complete(self, phase):
        attempt = self.engine.prepare(phase)
        self.engine.execute(phase, attempt, self.runner)
        self.assertEqual(self.engine.reconcile(phase, attempt, self.verifier, self.inspector), 'VERIFIED')
        return attempt

    def test_all_13_phases_interruption_resume_cleanup_and_finalization(self):
        for phase in e.PHASES:
            if phase != 'INSTALLED':
                self.complete(phase)
                continue
            attempt = self.engine.prepare(phase)
            def crash(point):
                if point == 'AFTER_EFFECT':
                    raise RuntimeError('power loss')
            with self.assertRaises(RuntimeError):
                self.engine.execute(phase, attempt, self.runner, fault=crash)
            self.engine = e.FixtureExecution(self.path, self.ctx)
            self.fails('AUTOMATIC_REPLAY_FORBIDDEN', self.engine.execute, phase, attempt, self.runner)
            self.assertEqual(self.engine.reconcile(phase, attempt, self.verifier, self.inspector), 'VERIFIED')
        state = self.engine.status()
        self.assertEqual(state['mode'], 'FIXTURE_ONLY')
        self.assertEqual(state['status'], 'FINALIZED')
        self.assertEqual(len(self.runner.calls), 13)
        self.fails('RUN_TERMINAL', self.engine.prepare, 'PREFLIGHT')
        self.assertEqual(len(self.engine.audit_cleanup(self.inspector)), 10)

    def test_concurrent_reservation_and_duplicate_execution(self):
        def reserve(_):
            try:
                return self.engine.prepare('PREFLIGHT')
            except e.LifecycleError:
                return None
        with ThreadPoolExecutor(max_workers=8) as pool:
            results = list(pool.map(reserve, range(8)))
        attempts = [x for x in results if x]
        self.assertEqual(len(attempts), 1)
        self.engine.execute('PREFLIGHT', attempts[0], self.runner)
        self.fails('AUTOMATIC_REPLAY_FORBIDDEN', self.engine.execute, 'PREFLIGHT', attempts[0], self.runner)
        self.assertEqual(len(self.runner.calls), 1)

    def test_crash_boundaries_rollback_and_unknown_outcome(self):
        for point in ('AFTER_INTENT', 'AFTER_EFFECT', 'AFTER_RECONCILING'):
            with self.subTest(point=point):
                with tempfile.TemporaryDirectory() as directory:
                    engine = e.FixtureExecution(Path(directory) / self.path.name, self.ctx)
                    engine.initialize(self.inventory)
                    attempt = engine.prepare('PREFLIGHT')
                    def crash(at):
                        if at == point:
                            raise RuntimeError('crash')
                    with self.assertRaises(RuntimeError):
                        engine.execute('PREFLIGHT', attempt, self.runner, fault=crash)
                    self.fails('AUTOMATIC_REPLAY_FORBIDDEN', engine.execute, 'PREFLIGHT', attempt, self.runner)
                    original = self.verifier.requery
                    with patch.object(self.verifier, 'requery', side_effect=lambda *args:
                                      {**original(*args), 'outcome': 'UNKNOWN'}):
                        self.assertEqual(engine.reconcile('PREFLIGHT', attempt, self.verifier, self.inspector), 'BLOCKED')
                    self.assertEqual(engine.reconcile('PREFLIGHT', attempt, self.verifier, self.inspector), 'VERIFIED')

    def test_verification_crash_rolls_back_and_requeries(self):
        attempt = self.engine.prepare('PREFLIGHT')
        self.engine.execute('PREFLIGHT', attempt, self.runner)
        def crash(_):
            raise RuntimeError('crash')
        with self.assertRaises(RuntimeError):
            self.engine.reconcile('PREFLIGHT', attempt, self.verifier, self.inspector, fault=crash)
        self.assertEqual(self.engine.status()['phases'][0]['state'], 'RECONCILING')
        self.assertEqual(self.engine.reconcile('PREFLIGHT', attempt, self.verifier, self.inspector), 'VERIFIED')

    def test_confirmed_failure_is_terminal_for_phase(self):
        attempt = self.engine.prepare('PREFLIGHT')
        self.engine.execute('PREFLIGHT', attempt, self.runner)
        original = self.verifier.requery
        with patch.object(self.verifier, 'requery', side_effect=lambda *args:
                          {**original(*args), 'outcome': 'CONFIRMED_FAILURE'}):
            self.assertEqual(self.engine.reconcile('PREFLIGHT', attempt, self.verifier, self.inspector), 'FAILED')
        self.fails('RECONCILIATION_REQUIRED', self.engine.prepare, 'PREFLIGHT')
        self.engine.terminate()
        self.engine.audit_cleanup(self.inspector)

    def test_cross_context_replayed_and_untrusted_verifier(self):
        attempt = self.engine.prepare('PREFLIGHT')
        self.engine.execute('PREFLIGHT', attempt, self.runner)
        original = self.verifier.requery
        for changes in ({'context': '0'*64}, {'mode': 'WINDOWS_NATIVE'}, {'attempt': 'replay'},
                        {'verified': True}, {'reference': '../escape'}, {'outcome': 'PASS'}):
            with self.subTest(changes=changes), patch.object(self.verifier, 'requery',
                    side_effect=lambda *args: {**original(*args), **changes}):
                with self.assertRaises(e.LifecycleError):
                    self.engine.reconcile('PREFLIGHT', attempt, self.verifier, self.inspector)
        for changes in ({'run_id': '2'*32}, {'machine_sha256': '0'*64}):
            engine = e.FixtureExecution(self.path, dataclasses.replace(self.ctx, **changes))
            self.fails('JOURNAL_CONTEXT_OR_DIGEST_INVALID', engine.status)

    def test_out_of_order_cancel_expire_and_native_boundary(self):
        self.fails('PHASE_ORDER_INVALID', self.engine.prepare, 'INSTALLED')
        self.fails('PHASE_ORDER_INVALID', self.engine.prepare, 'EVIDENCE_FINALIZED')
        with patch.object(e, '_clock', return_value='2099-01-01T00:00:00+00:00'):
            self.fails('RUN_EXPIRED', self.engine.prepare, 'PREFLIGHT')
            self.engine.terminate('EXPIRED')
        self.fails('RUN_TERMINAL', self.engine.prepare, 'PREFLIGHT')
        self.engine.audit_cleanup(self.inspector)
        self.fails('P028_NATIVE_AUTHORITY_UNAVAILABLE', e.native_execute, verified=True)
        with patch.object(a.subprocess, 'run') as process:
            self.fails('P028_NATIVE_AUTHORITY_UNAVAILABLE', a.launch, None, None, None, None)
            process.assert_not_called()

    def test_corrupted_and_partial_journal(self):
        self.engine.prepare('PREFLIGHT')
        db = sqlite3.connect(self.path)
        db.execute('UPDATE state SET payload=?', ('partial',))
        db.commit()
        db.close()
        self.fails('JOURNAL_CORRUPT', self.engine.status)

    def test_uncommitted_write_rolls_back(self):
        db = sqlite3.connect(self.path)
        db.execute('UPDATE state SET payload=?', ('partial',))
        db.close()
        self.assertEqual(self.engine.status()['phases'], [])

    def test_cleanup_every_resource_and_unknown_blocks(self):
        for resource in self.inventory:
            for bad in ('PRESENT', 'UNKNOWN'):
                with self.subTest(resource=resource, bad=bad):
                    with patch.object(self.inspector, 'query', side_effect=lambda c, r:
                                      bad if r == resource else Inspector().query(c, r)):
                        self.fails('CLEANUP_BLOCKED', self.engine.audit_cleanup, self.inspector)
        self.fails('CLEANUP_INVENTORY_INCOMPLETE', e.verify_cleanup, self.ctx, self.inventory[1:], self.inspector)

    def test_incomplete_cleanup_prevents_checkpoint_and_finalization(self):
        for phase in e.PHASES[:11]:
            self.complete(phase)
        attempt = self.engine.prepare('CLEANUP_VERIFIED')
        self.engine.execute('CLEANUP_VERIFIED', attempt, self.runner)
        with patch.object(self.inspector, 'query', return_value='UNKNOWN'):
            self.fails('CLEANUP_BLOCKED', self.engine.reconcile, 'CLEANUP_VERIFIED', attempt, self.verifier, self.inspector)
        self.fails('RECONCILIATION_REQUIRED', self.engine.prepare, 'EVIDENCE_FINALIZED')
        self.engine.reconcile('CLEANUP_VERIFIED', attempt, self.verifier, self.inspector)
        attempt = self.engine.prepare('EVIDENCE_FINALIZED')
        self.engine.execute('EVIDENCE_FINALIZED', attempt, self.runner)
        original = self.verifier.requery
        with patch.object(self.verifier, 'requery', side_effect=lambda phase, *args:
                          {**original(phase, *args), 'outcome': 'UNKNOWN'} if phase == 'INSTALLED' else original(phase, *args)):
            self.fails('EVIDENCE_CHANGED_OR_MISSING', self.engine.reconcile, 'EVIDENCE_FINALIZED', attempt, self.verifier, self.inspector)
        self.assertEqual(self.engine.status()['status'], 'ACTIVE')

    def test_duplicate_reconciliation_and_replayed_reference(self):
        first = self.complete('PREFLIGHT')
        attempt = self.engine.prepare('CANDIDATE_VERIFIED')
        self.engine.execute('CANDIDATE_VERIFIED', attempt, self.runner)
        original = self.verifier.requery
        reference = original('PREFLIGHT', first, self.ctx)['reference']
        with patch.object(self.verifier, 'requery', side_effect=lambda *args:
                          {**original(*args), 'reference': reference}):
            self.fails('EVIDENCE_REPLAY', self.engine.reconcile, 'CANDIDATE_VERIFIED', attempt, self.verifier, self.inspector)
        self.engine.reconcile('CANDIDATE_VERIFIED', attempt, self.verifier, self.inspector)
        self.fails('RECONCILIATION_STATE_INVALID', self.engine.reconcile, 'CANDIDATE_VERIFIED', attempt, self.verifier, self.inspector)

    def test_concurrent_completion_only_one_winner(self):
        attempt = self.engine.prepare('PREFLIGHT')
        self.engine.execute('PREFLIGHT', attempt, self.runner)
        def reconcile(_):
            try:
                return self.engine.reconcile('PREFLIGHT', attempt, self.verifier, self.inspector)
            except e.LifecycleError:
                return 'REJECTED'
        with ThreadPoolExecutor(max_workers=4) as pool:
            results = list(pool.map(reconcile, range(4)))
        self.assertEqual(results.count('VERIFIED'), 1)
        self.assertEqual(results.count('REJECTED'), 3)

    def test_wrong_observer_identity_remains_rejected(self):
        from test_windows_native_lifecycle import make_observation
        from windows_native_lifecycle import Observation
        obs = make_observation('PREFLIGHT', self.ctx)
        bad = Observation(obs.phase, obs.context_sha256, obs.references, self.ctx.subject_sid_sha256)
        self.fails('OBSERVER_IDENTITY_MISMATCH', bad.validate, self.ctx)

    def test_candidate_inputs_require_native_absolute_paths(self):
        root = Path(self.tmp.name).resolve()
        inputs = a.CandidateInputs(root / 'setup.exe', root / 'manifest.json', root / 'evidence', 123)
        inputs.validate()
        # Rooted POSIX spelling is not a drive-qualified Windows absolute path.
        self.assertTrue(PurePosixPath('/candidate/setup.exe').is_absolute())
        self.assertFalse(PureWindowsPath('/candidate/setup.exe').is_absolute())
        for field in ('setup', 'manifest', 'evidence_directory'):
            for invalid in (Path('relative'), Path('.'), Path(''),
                            root / 'bad\nname', root / 'bad\rname', root / 'bad\0name',
                            str(root / 'not-a-Path')):
                with self.subTest(field=field, invalid=repr(invalid)):
                    self.fails('ADAPTER_PATH_INVALID', dataclasses.replace(inputs, **{field: invalid}).validate)
        rooted = dataclasses.replace(inputs, setup=Path('/candidate/setup.exe'))
        if Path('/candidate/setup.exe').is_absolute():
            rooted.validate()
        else:
            self.fails('ADAPTER_PATH_INVALID', rooted.validate)
        # Drive-relative and root-relative forms must fail on actual Windows.
        if root.drive:
            for invalid in (Path(root.drive + 'relative.exe'), Path('\\candidate\\setup.exe')):
                self.fails('ADAPTER_PATH_INVALID', dataclasses.replace(inputs, setup=invalid).validate)

    def test_real_adapter_argument_contracts(self):
        fixture_root = Path(self.tmp.name).resolve()
        inputs = a.CandidateInputs(fixture_root / 'candidate' / 'setup.exe',
                                   fixture_root / 'candidate' / 'manifest.json',
                                   fixture_root / 'evidence', 123)
        for kind in a.CONTRACTS:
            args = a.build_arguments(kind, Path(__file__).resolve().parent.parent, self.ctx, inputs,
                approval='I_APPROVE_DISPOSABLE_SYNVEIL_WINDOWS_ACCEPTANCE', test_user='synveil_p013_1234abcd')
            self.assertIn(self.ctx.candidate_sha256, args)
            self.assertIn(self.ctx.source_commit, args)
            for flag, expected in (('-Setup', inputs.setup), ('-ReleaseManifest', inputs.manifest),
                                   ('-EvidenceDirectory', inputs.evidence_directory)):
                self.assertEqual(args[args.index(flag) + 1], str(expected))
            self.assertTrue(Path(args[5]).is_file())
        self.fails('HOST_MUTATION_APPROVAL_REQUIRED', a.build_arguments, 'journey', Path('/'), self.ctx, inputs, approval='yes')
        self.fails('DISPOSABLE_SUBJECT_INVALID', a.build_arguments, 'cleanup', Path('/'), self.ctx, inputs,
                   approval='I_APPROVE_DISPOSABLE_SYNVEIL_WINDOWS_ACCEPTANCE', test_user='Administrator')


if __name__ == '__main__':
    unittest.main()
