#!/usr/bin/env python3
"""P028 durable execution engine. Portable execution is always FIXTURE_ONLY.

SQLite FULL/DELETE transactions serialize reservations, not external effects.
An EXECUTING reservation is never stolen: after a crash/timeout the runner may
still be alive. Recovery only re-queries; it never reruns an operation. The
fixture database is not protected authority and cannot detect malicious whole
file rollback. Native use requires protected monotonic state and remains closed.
"""
from __future__ import annotations

from contextlib import contextmanager
from dataclasses import asdict, dataclass
import hashlib
import json
import os
from pathlib import Path
import secrets
import sqlite3
from typing import Protocol

from windows_native_lifecycle import PHASES, RunContext, LifecycleError, _require, _clock, _utc

KINDS = frozenset({'account', 'profile', 'installation', 'task', 'process', 'filesystem',
                   'secret', 'protected_state', 'pending_operation'})


def encode(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'))


def digest(value):
    return hashlib.sha256(encode(value).encode()).hexdigest()


@dataclass(frozen=True)
class Resource:
    kind: str
    reference_sha256: str
    disposition: str = 'DELETE'


class ResourceInspector(Protocol):
    def query(self, context: RunContext, resource: Resource) -> str:
        """Fresh independent query: ABSENT, PRESENT, RETAINED_PROTECTED or UNKNOWN."""


def verify_cleanup(context, inventory, inspector):
    """No deletion. A script exit code cannot satisfy this protocol.

    Inventory is captured before effects, includes explicit ABSENT queries for
    unused resource classes, and is immutable for the run. Audit evidence is
    RETAIN_PROTECTED; transient protected state must be DELETE.
    """
    _require(0 < len(inventory) <= 256 and {r.kind for r in inventory} == KINDS,
             'CLEANUP_INVENTORY_INCOMPLETE')
    _require(len({(r.kind, r.reference_sha256) for r in inventory}) == len(inventory),
             'CLEANUP_INVENTORY_DUPLICATE')
    statuses = []
    for resource in inventory:
        _require(len(resource.reference_sha256) == 64 and
                 all(c in '0123456789abcdef' for c in resource.reference_sha256), 'RESOURCE_REFERENCE_INVALID')
        _require(resource.disposition in {'DELETE', 'RETAIN_PROTECTED'} and
                 (resource.disposition != 'RETAIN_PROTECTED' or resource.kind == 'protected_state'),
                 'RESOURCE_DISPOSITION_INVALID')
        try:
            status = inspector.query(context, resource)
        except Exception:
            status = 'UNKNOWN'
        expected = 'ABSENT' if resource.disposition == 'DELETE' else 'RETAINED_PROTECTED'
        _require(status == expected, 'CLEANUP_BLOCKED')
        statuses.append({**asdict(resource), 'status': status})
    return statuses


class FixtureRunner(Protocol):
    def execute(self, phase: str, attempt: str, context: RunContext) -> None: ...


class FixtureVerifier(Protocol):
    def requery(self, phase: str, attempt: str, context: RunContext) -> dict:
        """Return fresh synthetic evidence; never interpreted as native proof."""


class FixtureExecution:
    """Executable fixture coordinator with isolated state and no native hooks.

    Verifier/runner are dependency-injected trusted test code, not serialized
    objects or user-selected commands. No such injection exists on native path.
    Each completion binds context, phase, attempt and a unique evidence reference.
    """
    def __init__(self, path: Path, context: RunContext):
        self.path, self.context = Path(path), context
        context.validate()
        _require(self.path.name == 'p028-execution-fixture.sqlite3' and self.path.is_absolute(),
                 'FIXTURE_PATH_INVALID')

    @contextmanager
    def transaction(self):
        _require(self.path.is_file() and not self.path.is_symlink() and
                 not any(p.is_symlink() for p in self.path.parents), 'FIXTURE_PATH_INVALID')
        _require(self.path.stat().st_size <= 16 * 1024 * 1024, 'JOURNAL_TOO_LARGE')
        db = sqlite3.connect(self.path, timeout=5, isolation_level=None)
        try:
            db.execute('PRAGMA synchronous=FULL')
            db.execute('BEGIN IMMEDIATE')
            yield db
            db.commit()
        except sqlite3.DatabaseError:
            raise LifecycleError('JOURNAL_CORRUPT_OR_BUSY') from None
        finally:
            if db.in_transaction:
                db.rollback()
            db.close()

    def initialize(self, inventory):
        # Validate the entire inventory without trusting a cleanup script.
        verify_cleanup(self.context, inventory, _AbsentInspector())
        _require(self.path.parent.is_dir() and not any(p.is_symlink() for p in self.path.parents),
                 'FIXTURE_PATH_INVALID')
        fd = os.open(self.path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
        os.close(fd)
        with self.transaction() as db:
            db.execute('CREATE TABLE state (id INTEGER PRIMARY KEY CHECK(id=1), payload TEXT, sha TEXT)')
            state = {'mode': 'FIXTURE_ONLY', 'context': asdict(self.context), 'status': 'ACTIVE',
                     'inventory': [asdict(r) for r in inventory], 'phases': []}
            db.execute('INSERT INTO state VALUES (1,?,?)', (encode(state), digest(state)))
        # SQLite syncs data and journal; sync the new directory entry on POSIX.
        if os.name == 'posix':
            fd = os.open(self.path.parent, os.O_RDONLY)
            try:
                os.fsync(fd)
            finally:
                os.close(fd)

    def load(self, db):
        row = db.execute('SELECT payload,sha FROM state WHERE id=1').fetchone()
        _require(row is not None and len(row[0]) <= 1024 * 1024, 'JOURNAL_CORRUPT')
        try:
            state = json.loads(row[0])
            _require(digest(state) == row[1] and state['mode'] == 'FIXTURE_ONLY' and
                     state['context'] == asdict(self.context), 'JOURNAL_CONTEXT_OR_DIGEST_INVALID')
            rows = state['phases']
            _require(len(rows) <= len(PHASES), 'JOURNAL_ORDER_INVALID')
            seen = set()
            for i, item in enumerate(rows):
                _require(item['phase'] == PHASES[i] and item['state'] in
                         {'PREPARED', 'EXECUTING', 'RECONCILING', 'VERIFIED', 'BLOCKED', 'FAILED'},
                         'JOURNAL_ORDER_INVALID')
                _require(i == len(rows)-1 or item['state'] == 'VERIFIED', 'JOURNAL_ORDER_INVALID')
                if item['state'] == 'VERIFIED':
                    self.check_receipt(item['receipt'], item)
                    _require(item['receipt']['outcome'] == 'CONFIRMED_SUCCESS', 'JOURNAL_RECEIPT_INVALID')
                    if item['phase'] in {'CLEANUP_VERIFIED', 'EVIDENCE_FINALIZED'}:
                        expected = verify_cleanup(self.context, [Resource(**r) for r in state['inventory']], _AbsentInspector())
                        _require(item.get('cleanup') == expected, 'CLEANUP_CHECKPOINT_INVALID')
                    ref = item['receipt']['reference']
                    _require(ref not in seen, 'EVIDENCE_REPLAY')
                    seen.add(ref)
            _require(state['status'] in {'ACTIVE', 'CANCELLED', 'EXPIRED', 'FINALIZED'}, 'JOURNAL_STATUS_INVALID')
            _require((state['status'] == 'FINALIZED') ==
                     (len(rows) == len(PHASES) and rows[-1]['state'] == 'VERIFIED'), 'PREMATURE_FINALIZATION')
            return state
        except (KeyError, TypeError, json.JSONDecodeError):
            raise LifecycleError('JOURNAL_CORRUPT') from None

    def save(self, db, state):
        db.execute('UPDATE state SET payload=?,sha=? WHERE id=1', (encode(state), digest(state)))

    def active(self, state):
        _require(state['status'] == 'ACTIVE', 'RUN_TERMINAL')
        _require(_utc(_clock()) < _utc(self.context.expires_at_utc), 'RUN_EXPIRED')

    def status(self):
        with self.transaction() as db:
            return self.load(db)

    def terminate(self, reason='CANCELLED'):
        _require(reason in {'CANCELLED', 'EXPIRED'}, 'TERMINATION_INVALID')
        with self.transaction() as db:
            state = self.load(db)
            _require(state['status'] == 'ACTIVE', 'RUN_TERMINAL')
            if reason == 'EXPIRED':
                _require(_utc(_clock()) >= _utc(self.context.expires_at_utc), 'RUN_NOT_EXPIRED')
            state['status'] = reason
            self.save(db, state)
        # Pending effects and inventory remain for independent cleanup, even when terminal.

    def prepare(self, phase):
        with self.transaction() as db:
            state = self.load(db)
            self.active(state)
            rows = state['phases']
            _require(not rows or rows[-1]['state'] == 'VERIFIED', 'RECONCILIATION_REQUIRED')
            _require(len(rows) < len(PHASES) and phase == PHASES[len(rows)], 'PHASE_ORDER_INVALID')
            attempt = secrets.token_hex(16)
            rows.append({'phase': phase, 'attempt': attempt, 'state': 'PREPARED'})
            self.save(db, state)
            return attempt

    def execute(self, phase, attempt, runner, *, fault=lambda point: None):
        with self.transaction() as db:
            state = self.load(db)
            self.active(state)
            item = self.match(state, phase, attempt)
            _require(item['state'] == 'PREPARED', 'AUTOMATIC_REPLAY_FORBIDDEN')
            item['state'] = 'EXECUTING'
            self.save(db, state)
        fault('AFTER_INTENT')
        # Any exception/timeout leaves EXECUTING: outcome UNKNOWN, never failed.
        runner.execute(phase, attempt, self.context)
        fault('AFTER_EFFECT')
        with self.transaction() as db:
            state = self.load(db)
            self.active(state)
            item = self.match(state, phase, attempt)
            _require(item['state'] == 'EXECUTING', 'CONCURRENT_OPERATION')
            item['state'] = 'RECONCILING'
            self.save(db, state)
        fault('AFTER_RECONCILING')

    def match(self, state, phase, attempt):
        _require(bool(state['phases']), 'PHASE_NOT_PREPARED')
        item = state['phases'][-1]
        _require(item['phase'] == phase and item['attempt'] == attempt, 'ATTEMPT_MISMATCH')
        return item

    def check_receipt(self, receipt, item):
        _require(type(receipt) is dict and set(receipt) ==
                 {'mode', 'context', 'phase', 'attempt', 'reference', 'outcome'}, 'UNTRUSTED_VERIFIER_OUTPUT')
        _require(receipt['mode'] == 'FIXTURE_ONLY' and receipt['context'] == digest(asdict(self.context)) and
                 receipt['phase'] == item['phase'] and receipt['attempt'] == item['attempt'], 'EVIDENCE_CONTEXT_MISMATCH')
        _require(isinstance(receipt['reference'], str) and len(receipt['reference']) == 64 and
                 all(c in '0123456789abcdef' for c in receipt['reference']), 'EVIDENCE_REFERENCE_INVALID')
        _require(receipt['outcome'] in {'CONFIRMED_SUCCESS', 'CONFIRMED_FAILURE', 'UNKNOWN'},
                 'UNTRUSTED_VERIFIER_OUTPUT')

    def reconcile(self, phase, attempt, verifier, inspector, *, fault=lambda point: None):
        # Keep transaction across re-query: concurrent completion cannot win a race.
        # A recovered live operation must return UNKNOWN until independently quiescent.
        with self.transaction() as db:
            state = self.load(db)
            self.active(state)
            item = self.match(state, phase, attempt)
            _require(item['state'] in {'EXECUTING', 'RECONCILING', 'BLOCKED'}, 'RECONCILIATION_STATE_INVALID')
            receipt = verifier.requery(phase, attempt, self.context)
            self.check_receipt(receipt, item)
            _require(all(p.get('receipt', {}).get('reference') != receipt['reference']
                         for p in state['phases']), 'EVIDENCE_REPLAY')
            item['state'] = {'UNKNOWN': 'BLOCKED', 'CONFIRMED_FAILURE': 'FAILED',
                             'CONFIRMED_SUCCESS': 'VERIFIED'}[receipt['outcome']]
            if item['state'] == 'VERIFIED':
                if phase in {'CLEANUP_VERIFIED', 'EVIDENCE_FINALIZED'}:
                    item['cleanup'] = verify_cleanup(self.context,
                        [Resource(**r) for r in state['inventory']], inspector)
                item['receipt'] = receipt
                if phase == 'EVIDENCE_FINALIZED':
                    # Re-query the whole chain, not only the final receipt.
                    for prior in state['phases'][:-1]:
                        fresh = verifier.requery(prior['phase'], prior['attempt'], self.context)
                        _require(fresh == prior['receipt'], 'EVIDENCE_CHANGED_OR_MISSING')
                    state['status'] = 'FINALIZED'
            fault('DURING_FINALIZATION')
            self.save(db, state)
        return item['state']

    def audit_cleanup(self, inspector):
        # Available after cancellation/expiry/failure; no deletion or phase advancement.
        state = self.status()
        return verify_cleanup(self.context, [Resource(**r) for r in state['inventory']], inspector)


class _AbsentInspector:
    def query(self, context, resource):
        return 'ABSENT' if resource.disposition == 'DELETE' else 'RETAINED_PROTECTED'


def native_execute(*args, **kwargs):
    # No caller-provided verifier, approval flag, or fixture receipt can open this boundary.
    raise LifecycleError('P028_NATIVE_AUTHORITY_UNAVAILABLE')
