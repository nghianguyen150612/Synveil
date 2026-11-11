"""Fixture-only coordinator tests; these do not qualify Windows acceptance."""
import datetime as dt
import json
import multiprocessing
import os
import sqlite3
import tempfile
import unittest
from unittest import mock
from pathlib import Path

import windows_native_coordinator as c


def concurrent_step(path, evidence, queue):
    try:
        c.advance(Path(path), 'PREFLIGHT', evidence)
        queue.put('PASS')
    except c.CoordinatorError as exc:
        queue.put(str(exc))


class CoordinatorTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.path = Path(self.tmp.name) / 'p028-fixture-state.sqlite3'
        self.expiry = (dt.datetime.now(dt.timezone.utc) + dt.timedelta(hours=1)).isoformat()
        self.run = c.initialize(self.path, fixture=True, approval='I_APPROVE_DISPOSABLE_FIXTURE_ONLY', expires=self.expiry)

    def evidence(self, phase, run=None):
        return json.dumps({'run_id': run or self.run, 'phase': phase, 'result': 'PASS', 'mode': 'FIXTURE_ONLY'}).encode()

    def rejected(self, reason, fn, *args, **kwargs):
        with self.assertRaises(c.CoordinatorError) as error:
            fn(*args, **kwargs)
        self.assertEqual(str(error.exception), reason)

    def test_resume_and_reject_illegal_duplicate_and_cross_run(self):
        self.rejected('PHASE_ORDER_INVALID', c.advance, self.path, 'INSTALLED', self.evidence('INSTALLED'))
        c.advance(self.path, 'PREFLIGHT', self.evidence('PREFLIGHT'))
        self.assertEqual(c.read_state(self.path)['completed'], ['PREFLIGHT'])
        self.rejected('PHASE_DUPLICATE', c.advance, self.path, 'PREFLIGHT', self.evidence('PREFLIGHT'))
        self.rejected('RUN_MISMATCH', c.advance, self.path, 'CANDIDATE_VERIFIED', self.evidence('CANDIDATE_VERIFIED', '0' * 32))
        c.advance(self.path, 'CANDIDATE_VERIFIED', self.evidence('CANDIDATE_VERIFIED'))
        self.assertEqual(c.read_state(self.path)['native_result'], 'BLOCKED_UNVERIFIED_NATIVE_PROVENANCE')

    def test_no_native_authority_or_unapproved_initialization(self):
        other = self.path.parent / 'other' / 'p028-fixture-state.sqlite3'
        other.parent.mkdir()
        self.rejected('NATIVE_AUTHORITY_UNAVAILABLE', c.initialize, other, fixture=False, approval='I_APPROVE_DISPOSABLE_FIXTURE_ONLY', expires=self.expiry)
        self.rejected('FIXTURE_APPROVAL_REQUIRED', c.initialize, other, fixture=True, approval='yes', expires=self.expiry)
        self.assertFalse(other.exists())
        self.rejected('STATE_ALREADY_EXISTS', c.initialize, self.path, fixture=True, approval='I_APPROVE_DISPOSABLE_FIXTURE_ONLY', expires=self.expiry)

    def test_concurrent_advance_serialized(self):
        ctx = multiprocessing.get_context('spawn')
        q = ctx.Queue()
        workers = [ctx.Process(target=concurrent_step, args=(str(self.path), self.evidence('PREFLIGHT'), q)) for _ in range(2)]
        for worker in workers: worker.start()
        for worker in workers: worker.join(15)
        self.assertEqual([worker.exitcode for worker in workers], [0, 0])
        self.assertEqual(sorted([q.get(timeout=3), q.get(timeout=3)]), ['PASS', 'PHASE_DUPLICATE'])
        self.assertEqual(c.read_state(self.path)['completed'], ['PREFLIGHT'])

    def test_interrupted_transaction_rolls_back(self):
        db = sqlite3.connect(str(self.path), isolation_level=None)
        db.execute('BEGIN IMMEDIATE')
        db.execute('INSERT INTO phases VALUES (0, ?, ?, ?, ?)', ('PREFLIGHT', 'bad', b'partial', 'time'))
        db.close()
        self.assertEqual(c.read_state(self.path)['completed'], [])
        c.advance(self.path, 'PREFLIGHT', self.evidence('PREFLIGHT'))
        self.assertEqual(c.read_state(self.path)['completed'], ['PREFLIGHT'])

    def test_corrupt_evidence_blocks_resume(self):
        c.advance(self.path, 'PREFLIGHT', self.evidence('PREFLIGHT'))
        with sqlite3.connect(str(self.path)) as db:
            db.execute('UPDATE phases SET evidence=?', (b'tampered',))
        self.rejected('EVIDENCE_CORRUPT', c.read_state, self.path)
        self.rejected('EVIDENCE_CORRUPT', c.advance, self.path, 'CANDIDATE_VERIFIED', self.evidence('CANDIDATE_VERIFIED'))

    def test_cleanup_and_terminal_are_independent_and_single_use(self):
        for phase in c.PHASES[:-1]:
            c.advance(self.path, phase, self.evidence(phase))
        self.rejected('CLEANUP_NOT_VERIFIED', c.advance, self.path, c.PHASES[-1], self.evidence(c.PHASES[-1]))
        cleanup = json.dumps({'run_id': self.run, 'mode': 'FIXTURE_ONLY', 'result': 'PASS'}).encode()
        c.record_cleanup(self.path, cleanup)
        self.rejected('CLEANUP_DUPLICATE', c.record_cleanup, self.path, cleanup)
        c.advance(self.path, c.PHASES[-1], self.evidence(c.PHASES[-1]))
        self.assertEqual(c.read_state(self.path)['status'], 'FINALIZED')
        self.rejected('RUN_TERMINAL', c.advance, self.path, c.PHASES[-1], self.evidence(c.PHASES[-1]))
        self.rejected('RUN_TERMINAL', c.record_cleanup, self.path, cleanup)

    def test_database_can_be_deleted_after_successful_operations(self):
        c.read_state(self.path)
        c.advance(self.path, 'PREFLIGHT', self.evidence('PREFLIGHT'))
        cleanup = json.dumps({'run_id': self.run, 'mode': 'FIXTURE_ONLY', 'result': 'PASS'}).encode()
        c.record_cleanup(self.path, cleanup)
        self.assertTrue(self.path.unlink() is None)
        self.assertFalse(self.path.exists())

    def test_database_can_be_deleted_after_operation_errors(self):
        self.rejected('PHASE_ORDER_INVALID', c.advance, self.path, 'INSTALLED', self.evidence('INSTALLED'))
        self.rejected('CLEANUP_RESULT_INVALID', c.record_cleanup, self.path,
                      json.dumps({'run_id': self.run, 'mode': 'FIXTURE_ONLY', 'result': 'UNKNOWN'}).encode())
        self.assertTrue(self.path.unlink() is None)
        self.assertFalse(self.path.exists())

    def test_open_db_closes_handle_when_pragma_setup_fails(self):
        class FakeDb:
            closed = False
            def execute(self, _statement):
                raise sqlite3.OperationalError('pragma failure')
            def close(self):
                self.closed = True
        fake = FakeDb()
        with mock.patch.object(c.sqlite3, 'connect', return_value=fake):
            self.rejected('STATE_UNSAFE_OR_MISSING', c.open_db, self.path.parent / 'missing.db')
            # Existing file passes path preflight; fail during PRAGMA instead.
            with mock.patch.object(Path, 'is_file', return_value=True), \
                 mock.patch.object(Path, 'is_symlink', return_value=False):
                with self.assertRaises(sqlite3.OperationalError):
                    c.open_db(self.path)
        self.assertTrue(fake.closed)

    def test_sqlite_database_reopens_after_child_process_exit(self):
        ctx = multiprocessing.get_context('spawn')
        q = ctx.Queue()
        worker = ctx.Process(target=concurrent_step, args=(str(self.path), self.evidence('PREFLIGHT'), q))
        worker.start()
        worker.join(15)
        self.assertEqual(worker.exitcode, 0)
        self.assertEqual(q.get(timeout=3), 'PASS')
        reopened = sqlite3.connect(str(self.path), timeout=1)
        try:
            self.assertEqual(reopened.execute('SELECT name FROM phases').fetchone()[0], 'PREFLIGHT')
        finally:
            reopened.close()

    def test_unsafe_path_and_invalid_evidence(self):
        self.rejected('EVIDENCE_INVALID', c.advance, self.path, 'PREFLIGHT', b'garbage')
        self.rejected('EVIDENCE_SIZE_INVALID', c.advance, self.path, 'PREFLIGHT', b'')
        link = self.path.parent / 'state-link'
        try:
            os.symlink(self.path, link)
        except (OSError, NotImplementedError):
            self.skipTest('symlink creation unavailable')
        self.rejected('STATE_UNSAFE_OR_MISSING', c.read_state, link)


if __name__ == '__main__':
    unittest.main()
