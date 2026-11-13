"""Adversarial tests for the read-only native phase ledger auditor."""
import copy
import hashlib
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location('ledger', Path(__file__).with_name('verify-windows-native-phase-ledger.py'))
assert spec and spec.loader
ledger = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ledger)


class LedgerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.context = {'run_id': '1' * 32, 'source_commit': 'a' * 40, 'harness_commit': 'b' * 40,
                        'candidate_sha256': 'c' * 64, 'manifest_sha256': 'd' * 64,
                        'producer_run_id': '123', 'machine_sha256': 'e' * 64,
                        'user_sid_sha256': 'f' * 64, 'windows_build': 26100,
                        'architecture': 'x86_64', 'started_at_utc': '2026-01-01T00:00:00Z',
                        'expires_at_utc': '2026-01-02T00:00:00Z'}
        self.data = {'schema_version': 1, 'context': self.context, 'phases': []}
        for index, phase in enumerate(ledger.PHASES):
            time = f'2026-01-01T00:{index:02d}:00Z'
            item = {'schema_version': 1, 'phase': phase, 'context': self.context,
                    'result': 'PASS', 'completed_at_utc': time}
            name = f'phase{index}.json'
            raw = json.dumps(item).encode()
            (self.root / name).write_bytes(raw)
            self.data['phases'].append({'phase': phase, 'run_id': self.context['run_id'],
                                        'completed_at_utc': time, 'file': name,
                                        'sha256': hashlib.sha256(raw).hexdigest()})

    def reject(self, reason):
        with self.assertRaises(ledger.LedgerError) as error:
            ledger.verify(self.data, self.root)
        self.assertEqual(str(error.exception), reason)

    def mutate_item(self, index, mutation):
        entry = self.data['phases'][index]
        path = self.root / entry['file']
        item = json.loads(path.read_bytes())
        mutation(item)
        raw = json.dumps(item).encode()
        path.write_bytes(raw)
        entry['sha256'] = hashlib.sha256(raw).hexdigest()

    def test_valid_integrity_never_claims_native_acceptance(self):
        self.assertEqual(ledger.verify(self.data, self.root), 'BLOCKED_UNVERIFIED_NATIVE_PROVENANCE')

    def test_context_mismatch_is_rejected(self):
        for field, value in [('run_id', '9' * 32), ('machine_sha256', '9' * 64),
                             ('user_sid_sha256', '9' * 64), ('candidate_sha256', '9' * 64),
                             ('manifest_sha256', '9' * 64), ('harness_commit', '9' * 40),
                             ('source_commit', '9' * 40), ('producer_run_id', '999')]:
            with self.subTest(field=field):
                self.setUp()
                self.mutate_item(3, lambda item: item['context'].__setitem__(field, value))
                self.reject('EVIDENCE_CONTEXT_MISMATCH')

    def test_phase_and_evidence_attacks(self):
        cases = [
            ('out of order', lambda: self.data['phases'][1].__setitem__('phase', 'INSTALLED'), 'PHASE_ORDER_INVALID'),
            ('duplicate phase', lambda: self.data['phases'][1].__setitem__('phase', 'PREFLIGHT'), 'PHASE_ORDER_INVALID'),
            ('cross run', lambda: self.data['phases'][0].__setitem__('run_id', '9' * 32), 'RUN_MISMATCH'),
            ('stale', lambda: self.data['phases'][1].__setitem__('completed_at_utc', '2025-01-01T00:00:00Z'), 'PHASE_TIME_INVALID'),
            ('reused file', lambda: self.data['phases'][1].__setitem__('file', 'phase0.json'), 'EVIDENCE_REUSED'),
            ('outside root', lambda: self.data['phases'][0].__setitem__('file', '../other.json'), 'EVIDENCE_PATH_INVALID'),
            ('bad hash', lambda: self.data['phases'][0].__setitem__('sha256', '0' * 64), 'EVIDENCE_DIGEST_MISMATCH'),
            ('deleted', lambda: (self.root / 'phase0.json').unlink(), 'EVIDENCE_MISSING_OR_UNSAFE'),
            ('tampered', lambda: (self.root / 'phase0.json').write_text('{}'), 'EVIDENCE_DIGEST_MISMATCH'),
            ('blocked', lambda: self.mutate_item(0, lambda i: i.__setitem__('result', 'BLOCKED')), 'PHASE_NOT_PASS'),
            ('wrong phase', lambda: self.mutate_item(0, lambda i: i.__setitem__('phase', 'INSTALLED')), 'EVIDENCE_PHASE_MISMATCH'),
            ('arch', lambda: self.context.__setitem__('architecture', 'arm64'), 'PLATFORM_UNSUPPORTED'),
            ('schema', lambda: self.data.__setitem__('schema_version', 2), 'SCHEMA_UNSUPPORTED'),
            ('invalid run', lambda: self.context.__setitem__('run_id', 'x'), 'RUN_ID_INVALID'),
        ]
        for name, attack, reason in cases:
            with self.subTest(name=name):
                self.setUp()
                attack()
                self.reject(reason)

    def test_partial_run_is_blocked(self):
        self.data['phases'] = self.data['phases'][:2]
        self.assertEqual(ledger.verify(self.data, self.root), 'BLOCKED_UNVERIFIED_NATIVE_PROVENANCE')


if __name__ == '__main__':
    unittest.main()
