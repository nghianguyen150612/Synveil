#!/usr/bin/env python3
"""Fail-closed P028 lifecycle adapter contracts and pending native journal.

This module records bounded orchestration checkpoints. It is deliberately not
a Windows trust authority: observations must be independently re-queried and
verified, and native phase advancement remains unavailable.
"""
from __future__ import annotations

from dataclasses import asdict, dataclass
from contextlib import contextmanager
import datetime as dt
import hashlib
import json
import os
from pathlib import Path
import secrets
import sqlite3
import stat
from typing import Protocol, Sequence

PHASES = (
    'PREFLIGHT', 'CANDIDATE_VERIFIED', 'ENVIRONMENT_QUALIFIED', 'INSTALLED',
    'GUI_VERIFIED', 'IPC_VERIFIED', 'STARTUP_CONFIGURED', 'LOGOFF_OBSERVED',
    'NEW_LOGON_OBSERVED', 'LOGON_TRIGGER_VERIFIED', 'LIFECYCLE_VERIFIED',
    'CLEANUP_VERIFIED', 'EVIDENCE_FINALIZED',
)
MAX_REFERENCES = 256
MAX_REFERENCE_BYTES = 65536
MAX_JOURNAL_BYTES = 16 * 1024 * 1024
NATIVE_JOURNAL_NAME = 'p028-native-lifecycle.sqlite3'


class LifecycleError(ValueError):
    """Stable, redacted P028 lifecycle failure."""


def _require(ok: bool, reason: str) -> None:
    if not ok:
        raise LifecycleError(reason)


def _utc(value: str) -> dt.datetime:
    try:
        parsed = dt.datetime.fromisoformat(value.replace('Z', '+00:00'))
    except (ValueError, AttributeError):
        raise LifecycleError('TIMESTAMP_INVALID') from None
    _require(parsed.tzinfo is not None and parsed.utcoffset() == dt.timedelta(0), 'TIMESTAMP_INVALID')
    return parsed.astimezone(dt.timezone.utc)


def _digest(value: str) -> bool:
    return isinstance(value, str) and len(value) == 64 and all(c in '0123456789abcdef' for c in value)


@dataclass(frozen=True)
class RunContext:
    run_id: str
    machine_sha256: str
    boot_sha256: str
    source_commit: str
    harness_commit: str
    candidate_sha256: str
    manifest_sha256: str
    controller_sid_sha256: str
    subject_sid_sha256: str
    observer_sid_sha256: str
    created_at_utc: str
    expires_at_utc: str

    def validate(self) -> None:
        _require(len(self.run_id) == 32 and all(c in '0123456789abcdef' for c in self.run_id), 'RUN_ID_INVALID')
        for name in ('machine_sha256', 'boot_sha256', 'candidate_sha256', 'manifest_sha256',
                     'controller_sid_sha256', 'subject_sid_sha256', 'observer_sid_sha256'):
            _require(_digest(getattr(self, name)), 'CONTEXT_DIGEST_INVALID')
        for name in ('source_commit', 'harness_commit'):
            value = getattr(self, name)
            _require(len(value) == 40 and all(c in '0123456789abcdef' for c in value), 'CONTEXT_COMMIT_INVALID')
        _require(len({self.controller_sid_sha256, self.subject_sid_sha256, self.observer_sid_sha256}) == 3,
                 'ROLE_IDENTITY_COLLISION')
        created, expires = _utc(self.created_at_utc), _utc(self.expires_at_utc)
        _require(created < expires <= created + dt.timedelta(days=7), 'RUN_DEADLINE_INVALID')

    def canonical(self) -> bytes:
        self.validate()
        return json.dumps(asdict(self), sort_keys=True, separators=(',', ':')).encode('utf-8')


@dataclass(frozen=True)
class EvidenceReference:
    """A bounded OS reference, not proof until an independent verifier re-queries it."""

    source: str
    event_id: int
    record_id: int
    record_sha256: str
    occurred_at_utc: str
    machine_sha256: str
    boot_sha256: str
    run_id: str
    task_instance_sha256: str = ''
    process_id: int = 0
    process_created_filetime: int = 0
    subject_sid_sha256: str = ''
    session_id: int = 0
    authentication_luid_sha256: str = ''
    logon_type: int = 0

    def validate(self, context: RunContext, phase: str) -> None:
        _require(phase in PHASES, 'PHASE_INVALID')
        _require(self.source in {'Security', 'TaskScheduler'}, 'EVIDENCE_SOURCE_UNSUPPORTED')
        allowed = {
            'Security': {4624, 4634, 4688},
            'TaskScheduler': {119, 100, 129, 200},
        }
        _require(self.event_id in allowed[self.source] and self.record_id > 0, 'EVENT_REFERENCE_INVALID')
        _require(_digest(self.record_sha256) and self.machine_sha256 == context.machine_sha256 and
                 self.boot_sha256 == context.boot_sha256 and self.run_id == context.run_id,
                 'EVIDENCE_CONTEXT_MISMATCH')
        occurred = _utc(self.occurred_at_utc)
        _require(_utc(context.created_at_utc) <= occurred <= _utc(context.expires_at_utc),
                 'EVIDENCE_TIME_OUT_OF_RUN')
        _require(self.process_id >= 0 and self.process_created_filetime >= 0, 'PROCESS_REFERENCE_INVALID')
        if self.source == 'TaskScheduler' and self.event_id in {119, 100, 129, 200}:
            _require(_digest(self.task_instance_sha256), 'TASK_INSTANCE_REFERENCE_INVALID')
        if self.source == 'Security':
            _require(self.subject_sid_sha256 == context.subject_sid_sha256, 'SUBJECT_IDENTITY_MISMATCH')
            if self.event_id in {4624, 4634, 4688}:
                _require(self.session_id > 0 and _digest(self.authentication_luid_sha256),
                         'SESSION_AUTHENTICATION_CONTEXT_INVALID')
            if self.event_id == 4624:
                _require(self.logon_type in {2, 10}, 'INTERACTIVE_LOGON_TYPE_INVALID')
        if self.source == 'TaskScheduler' and self.event_id == 129:
            _require(self.session_id > 0 and _digest(self.authentication_luid_sha256),
                     'SESSION_AUTHENTICATION_CONTEXT_INVALID')
        if self.event_id == 129 or self.event_id == 4688:
            _require(self.process_id > 0 and self.process_created_filetime > 0, 'PROCESS_REFERENCE_INVALID')


@dataclass(frozen=True)
class Observation:
    phase: str
    context_sha256: str
    references: tuple[EvidenceReference, ...]
    observer_identity_sha256: str

    def validate(self, context: RunContext) -> None:
        _require(self.phase in PHASES and self.context_sha256 == hashlib.sha256(context.canonical()).hexdigest(),
                 'OBSERVATION_CONTEXT_MISMATCH')
        _require(self.observer_identity_sha256 == context.observer_sid_sha256, 'OBSERVER_IDENTITY_MISMATCH')
        _require(0 < len(self.references) <= MAX_REFERENCES, 'EVIDENCE_REFERENCE_COUNT_INVALID')
        seen: set[tuple[str, int]] = set()
        for ref in self.references:
            ref.validate(context, self.phase)
            key = (ref.source, ref.record_id)
            _require(key not in seen, 'EVIDENCE_REFERENCE_DUPLICATE')
            seen.add(key)
        keys = {(ref.source, ref.event_id) for ref in self.references}
        required = {
            'LOGOFF_OBSERVED': {('Security', 4634)},
            'NEW_LOGON_OBSERVED': {('Security', 4624)},
            'LOGON_TRIGGER_VERIFIED': {
                ('Security', 4624), ('Security', 4634), ('Security', 4688),
                ('TaskScheduler', 119), ('TaskScheduler', 100),
                ('TaskScheduler', 129), ('TaskScheduler', 200),
            },
        }.get(self.phase, set())
        _require(required <= keys, 'EVIDENCE_REFERENCE_SET_INCOMPLETE')
        if self.phase == 'LOGON_TRIGGER_VERIFIED':
            security = [ref for ref in self.references if ref.source == 'Security']
            scheduler = [ref for ref in self.references if ref.source == 'TaskScheduler']
            logons = sorted((ref for ref in security if ref.event_id == 4624), key=lambda ref: _utc(ref.occurred_at_utc))
            logoffs = [ref for ref in security if ref.event_id == 4634]
            audits = [ref for ref in security if ref.event_id == 4688]
            _require(len(logons) == 2 and len(logoffs) == 1 and len(audits) == 1,
                     'LOGON_TRIGGER_SECURITY_RECORD_SET_INVALID')
            initial, subsequent = logons
            logoff = logoffs[0]
            _require(initial.record_id != subsequent.record_id and
                     initial.authentication_luid_sha256 != subsequent.authentication_luid_sha256,
                     'AUTHENTICATION_SESSION_REPLAY')
            _require(initial.session_id == logoff.session_id and
                     initial.authentication_luid_sha256 == logoff.authentication_luid_sha256 and
                     _utc(initial.occurred_at_utc) <= _utc(logoff.occurred_at_utc) < _utc(subsequent.occurred_at_utc),
                     'LOGOFF_NEW_LOGON_SEQUENCE_INVALID')
            by_scheduler_id = {event_id: [ref for ref in scheduler if ref.event_id == event_id]
                               for event_id in (119, 100, 129, 200)}
            _require(all(len(records) == 1 for records in by_scheduler_id.values()),
                     'TASK_INSTANCE_AMBIGUOUS')
            trigger, started, process_created, action = (by_scheduler_id[event_id][0]
                                                         for event_id in (119, 100, 129, 200))
            instance_ids = {ref.task_instance_sha256 for ref in (trigger, started, process_created, action)}
            _require(len(instance_ids) == 1, 'TASK_INSTANCE_CONTEXT_MISMATCH')
            _require(_utc(subsequent.occurred_at_utc) < _utc(trigger.occurred_at_utc) <
                     _utc(started.occurred_at_utc) < _utc(process_created.occurred_at_utc) <
                     _utc(action.occurred_at_utc), 'TASK_EVENT_ORDER_INVALID')
            audit = audits[0]
            _require(process_created.process_id == audit.process_id and
                     process_created.process_created_filetime == audit.process_created_filetime and
                     process_created.session_id == subsequent.session_id == audit.session_id and
                     process_created.authentication_luid_sha256 == subsequent.authentication_luid_sha256 ==
                     audit.authentication_luid_sha256,
                     'TASK_PROCESS_SESSION_MISMATCH')
        _require(len(self.to_bytes()) <= MAX_REFERENCE_BYTES, 'EVIDENCE_REFERENCE_SIZE_INVALID')

    def to_bytes(self) -> bytes:
        return json.dumps({
            'phase': self.phase,
            'context_sha256': self.context_sha256,
            'observer_identity_sha256': self.observer_identity_sha256,
            'references': [asdict(ref) for ref in self.references],
        }, sort_keys=True, separators=(',', ':')).encode('utf-8')


@dataclass(frozen=True)
class VerificationResult:
    """Verifier output is a request to authority, never phase authorization by itself."""

    verified: bool
    verifier_identity_sha256: str
    requery_count: int
    reason: str

    def validate(self, context: RunContext | None = None) -> None:
        _require(_digest(self.verifier_identity_sha256), 'VERIFIER_IDENTITY_INVALID')
        _require(0 <= self.requery_count <= MAX_REFERENCES, 'VERIFIER_REQUERY_COUNT_INVALID')
        _require(self.reason in {'VERIFIED', 'MISSING_RECORD', 'RECORD_CHANGED', 'AMBIGUOUS',
                                 'UNSUPPORTED', 'CONTEXT_MISMATCH', 'API_UNAVAILABLE'},
                 'VERIFIER_REASON_INVALID')
        _require((self.verified and self.reason == 'VERIFIED') or not self.verified, 'VERIFIER_RESULT_INVALID')
        if context is not None:
            _require(self.verifier_identity_sha256 not in {
                context.controller_sid_sha256, context.subject_sid_sha256, context.observer_sid_sha256,
            }, 'VERIFIER_ROLE_COLLISION')


class Observer(Protocol):
    def observe(self, phase: str, context: RunContext) -> Observation: ...


class IndependentVerifier(Protocol):
    def verify(self, phase: str, context: RunContext, observation: Observation) -> VerificationResult: ...


class TrustedPhaseAdvancer(Protocol):
    def advance(self, phase: str, context: RunContext, observation: Observation,
                verification: VerificationResult) -> None: ...


class NativeAuthorityUnavailable:
    """The only production adapter until native trust qualification passes."""

    def advance(self, phase: str, context: RunContext, observation: Observation,
                verification: VerificationResult) -> None:
        raise LifecycleError('P028_NATIVE_AUTHORITY_UNAVAILABLE')


def orchestration_plan(mode: str = 'FIXTURE_ONLY') -> dict:
    """Return a bounded decision plan; never launches lifecycle or OS mutations."""
    _require(mode in {'FIXTURE_ONLY', 'WINDOWS_NATIVE'}, 'MODE_INVALID')
    tools = {
        'PREFLIGHT': ('qualify-windows-native-environment.ps1', False, False),
        'CANDIDATE_VERIFIED': ('verify_release_candidate_run.py', False, False),
        'ENVIRONMENT_QUALIFIED': ('qualify-windows-native-environment.ps1', False, False),
        'INSTALLED': ('test-windows-gui-journey.ps1:ordinary-user Setup journey', True, True),
        'GUI_VERIFIED': ('test-windows-gui-journey.ps1', False, False),
        'IPC_VERIFIED': ('p013_windows_control_probe', False, False),
        'STARTUP_CONFIGURED': ('protect-windows-native-run-state.ps1:CaptureLogonTask', False, False),
        'LOGOFF_OBSERVED': ('protect-windows-native-run-state.ps1:ObserveLogoff', False, False),
        'NEW_LOGON_OBSERVED': ('protect-windows-native-run-state.ps1:ObserveNewLogon', False, False),
        'LOGON_TRIGGER_VERIFIED': ('protect-windows-native-run-state.ps1:ObserveLogonTrigger', False, False),
        'LIFECYCLE_VERIFIED': ('test-windows-installed-uninstall.ps1', True, True),
        'CLEANUP_VERIFIED': ('cleanup-windows-p013-test-user.ps1', True, True),
        'EVIDENCE_FINALIZED': ('independent-cleanup-and-evidence-verifier', False, False),
    }
    return {
        'mode': mode,
        'native_authority': 'BLOCKED',
        'native_result': 'P028_NATIVE_AUTHORITY_UNAVAILABLE',
        'steps': [
            {'position': index, 'phase': phase, 'tool': tools[phase][0],
             'explicit_approval_required': tools[phase][1],
             'mutates_host': tools[phase][2], 'state': 'NOT_RUN'}
            for index, phase in enumerate(PHASES)
        ],
        'automatic_replay': False,
    }


def simulate_fixture_plan(outcomes: dict[str, str], *, interrupted_at: str | None = None) -> dict:
    """Exercise ordered orchestration decisions without host calls or state writes.

    Outcomes are synthetic inputs. This helper has no route to NativePendingJournal
    or a phase advancer and its completion label can never be native acceptance.
    """
    _require(isinstance(outcomes, dict) and outcomes.keys() <= set(PHASES), 'FIXTURE_PHASE_INVALID')
    allowed = {'PASS', 'FAIL', 'BLOCKED', 'NOT_RUN'}
    _require(all(value in allowed for value in outcomes.values()), 'FIXTURE_OUTCOME_INVALID')
    _require(interrupted_at is None or interrupted_at in PHASES, 'FIXTURE_PHASE_INVALID')
    steps = []
    halted = False
    result = 'FIXTURE_ONLY_COMPLETE'
    for phase in PHASES:
        if halted:
            status = 'NOT_RUN'
        elif phase == interrupted_at:
            status = 'INTERRUPTED'
            halted = True
            result = 'FIXTURE_ONLY_INTERRUPTED'
        else:
            status = outcomes.get(phase, 'NOT_RUN')
            if status != 'PASS':
                halted = True
                result = 'FIXTURE_ONLY_INCOMPLETE'
                if any(outcomes.get(later, 'NOT_RUN') != 'NOT_RUN' for later in PHASES[PHASES.index(phase) + 1:]):
                    raise LifecycleError('FIXTURE_OUTCOME_AFTER_HALT')
        steps.append({'position': len(steps), 'phase': phase, 'status': status,
                      'evidence': 'SYNTHETIC_FIXTURE' if status in {'PASS', 'FAIL', 'BLOCKED'} else 'NONE',
                      'host_mutation': False})
    if interrupted_at is not None and outcomes.get(interrupted_at, 'NOT_RUN') != 'NOT_RUN':
        raise LifecycleError('FIXTURE_OUTCOME_AFTER_INTERRUPTION')
    return {'mode': 'FIXTURE_ONLY', 'result': result, 'native_authority': 'BLOCKED',
            'native_result': 'P028_NATIVE_AUTHORITY_UNAVAILABLE', 'steps': steps,
            'host_mutations_performed': False}


def _clock() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat(timespec='seconds')


def _check_db(path: Path) -> None:
    _require(path.is_absolute(), 'NATIVE_JOURNAL_PATH_UNSAFE')
    _require(path.name == NATIVE_JOURNAL_NAME, 'NATIVE_JOURNAL_PATH_UNSAFE')
    for parent in (path.parent, *path.parents):
        if parent.exists():
            _require(not parent.is_symlink() and parent.is_dir(), 'NATIVE_JOURNAL_PATH_UNSAFE')
    _require(path.parent.is_dir(), 'NATIVE_JOURNAL_PATH_UNSAFE')
    _require(not path.is_symlink(), 'NATIVE_JOURNAL_REPARSE_REJECTED')
    if path.exists():
        details = path.stat(follow_symlinks=False)
        _require(stat.S_ISREG(details.st_mode) and details.st_size <= MAX_JOURNAL_BYTES and details.st_nlink == 1,
                 'NATIVE_JOURNAL_UNSAFE')
        _require(details.st_ino != 0, 'NATIVE_JOURNAL_FILE_ID_UNSUPPORTED')


def _journal_identity(path: Path) -> tuple[int, int, int]:
    try:
        details = path.stat(follow_symlinks=False)
    except OSError:
        raise LifecycleError('NATIVE_JOURNAL_PATH_CHANGED') from None
    _require(stat.S_ISREG(details.st_mode) and details.st_ino != 0 and details.st_nlink == 1,
             'NATIVE_JOURNAL_PATH_CHANGED')
    return details.st_dev, details.st_ino, details.st_nlink


class NativePendingJournal:
    """Crash-safe pending checkpoints. It cannot accept or authorize a phase."""

    def __init__(self, path: Path):
        self.path = path

    @classmethod
    def initialize(cls, path: Path, context: RunContext, *, approval: str) -> 'NativePendingJournal':
        _require(approval == 'I_APPROVE_P028_NATIVE_PENDING_JOURNAL', 'NATIVE_JOURNAL_APPROVAL_REQUIRED')
        _check_db(path)
        context_bytes = context.canonical()
        try:
            fd = os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
        except FileExistsError:
            raise LifecycleError('NATIVE_JOURNAL_ALREADY_EXISTS') from None
        os.close(fd)
        try:
            with cls(path)._db() as db:
                db.execute('BEGIN IMMEDIATE')
                db.execute('CREATE TABLE meta (id INTEGER PRIMARY KEY CHECK (id=1), mode TEXT NOT NULL, context BLOB NOT NULL, context_sha256 TEXT NOT NULL, status TEXT NOT NULL, created_at_utc TEXT NOT NULL)')
                db.execute('CREATE TABLE checkpoint (position INTEGER PRIMARY KEY, phase TEXT UNIQUE NOT NULL, status TEXT NOT NULL, attempt_id TEXT NOT NULL, observation BLOB, observation_sha256 TEXT, verification BLOB, verification_sha256 TEXT, updated_at_utc TEXT NOT NULL)')
                db.execute('INSERT INTO meta VALUES (1, ?, ?, ?, ?, ?)',
                           ('NATIVE_PENDING_AUTHORITY', context_bytes,
                            hashlib.sha256(context_bytes).hexdigest(), 'ACTIVE_BLOCKED', _clock()))
                db.execute('COMMIT')
        except BaseException:
            # Keep any incomplete file for inspection. It is never treated as initialized.
            raise
        return cls(path)

    @contextmanager
    def _db(self):
        _check_db(self.path)
        before = _journal_identity(self.path)
        try:
            db = sqlite3.connect(str(self.path), timeout=5, isolation_level=None)
        except sqlite3.DatabaseError:
            raise LifecycleError('NATIVE_JOURNAL_CORRUPT_OR_BUSY') from None
        try:
            # Detect a leaf substitution between path inspection and SQLite's
            # pathname open before running a query or mutation. This does not
            # defeat an attacker who can swap the path and restore it between
            # checks; native use still requires protected parent ACLs/handles.
            _require(_journal_identity(self.path) == before, 'NATIVE_JOURNAL_PATH_CHANGED_DURING_OPEN')
        except BaseException:
            db.close()
            raise
        try:
            db.execute('PRAGMA foreign_keys=ON')
            db.execute('PRAGMA synchronous=FULL')
            yield db
            _require(_journal_identity(self.path) == before, 'NATIVE_JOURNAL_PATH_CHANGED_DURING_USE')
        except BaseException:
            if db.in_transaction:
                db.rollback()
            raise
        finally:
            db.close()

    def _context(self, db: sqlite3.Connection) -> RunContext:
        try:
            rows = db.execute('SELECT mode, context, context_sha256, status FROM meta').fetchall()
        except sqlite3.DatabaseError:
            raise LifecycleError('NATIVE_JOURNAL_CORRUPT') from None
        _require(len(rows) == 1 and rows[0][0] == 'NATIVE_PENDING_AUTHORITY' and
                 rows[0][3] == 'ACTIVE_BLOCKED', 'NATIVE_JOURNAL_CORRUPT')
        raw = rows[0][1]
        _require(hashlib.sha256(raw).hexdigest() == rows[0][2], 'NATIVE_CONTEXT_CORRUPT')
        try:
            context = RunContext(**json.loads(raw))
            context.validate()
        except (TypeError, ValueError, json.JSONDecodeError):
            raise LifecycleError('NATIVE_CONTEXT_CORRUPT') from None
        _require(dt.datetime.now(dt.timezone.utc) <= _utc(context.expires_at_utc), 'RUN_EXPIRED')
        return context

    def status(self) -> dict:
        try:
            with self._db() as db:
                context = self._context(db)
                rows = db.execute('SELECT position, phase, status, observation, observation_sha256, verification, verification_sha256 FROM checkpoint ORDER BY position').fetchall()
                for index, row in enumerate(rows):
                    _require(row[0] == index and index < len(PHASES) and row[1] == PHASES[index], 'NATIVE_CHECKPOINT_ORDER_INVALID')
                    if row[3] is not None:
                        _require(hashlib.sha256(row[3]).hexdigest() == row[4], 'NATIVE_OBSERVATION_CORRUPT')
                    if row[5] is not None:
                        _require(hashlib.sha256(row[5]).hexdigest() == row[6], 'NATIVE_VERIFICATION_CORRUPT')
                        try:
                            verification = VerificationResult(**json.loads(row[5]))
                            verification.validate(context)
                        except (TypeError, ValueError, json.JSONDecodeError):
                            raise LifecycleError('NATIVE_VERIFICATION_CORRUPT') from None
                        expected_state = 'VERIFIED_PENDING_AUTHORITY' if verification.verified else 'BLOCKED_VERIFICATION_FAILED'
                        _require(row[2] == expected_state, 'NATIVE_VERIFICATION_STATE_MISMATCH')
                    elif row[2] in {'VERIFIED_PENDING_AUTHORITY', 'BLOCKED_VERIFICATION_FAILED'}:
                        raise LifecycleError('NATIVE_VERIFICATION_MISSING')
                return {'mode': 'NATIVE_PENDING_AUTHORITY', 'run_id': context.run_id,
                        'status': 'ACTIVE_BLOCKED',
                        'pending': ({'phase': rows[-1][1], 'checkpoint_state': rows[-1][2]} if rows else None),
                        'completed_phases': [], 'native_authority': 'BLOCKED',
                        'native_result': 'P028_NATIVE_AUTHORITY_UNAVAILABLE'}
        except sqlite3.DatabaseError:
            raise LifecycleError('NATIVE_JOURNAL_CORRUPT_OR_BUSY') from None

    def begin_observation(self, phase: str) -> str:
        _require(phase in PHASES, 'PHASE_INVALID')
        try:
            with self._db() as db:
                db.execute('BEGIN IMMEDIATE')
                self._context(db)
                rows = db.execute('SELECT position, phase, status FROM checkpoint ORDER BY position').fetchall()
                for index, row in enumerate(rows):
                    _require(row[0] == index and row[1] == PHASES[index], 'NATIVE_CHECKPOINT_ORDER_INVALID')
                if rows:
                    _require(rows[-1][2] == 'AUTHORIZED', 'NATIVE_AUTHORITY_UNAVAILABLE')
                _require(len(rows) < len(PHASES) and phase == PHASES[len(rows)], 'PHASE_ORDER_INVALID')
                attempt = secrets.token_hex(16)
                db.execute('INSERT INTO checkpoint VALUES (?, ?, ?, ?, NULL, NULL, NULL, NULL, ?)',
                           (len(rows), phase, 'OBSERVATION_STARTED', attempt, _clock()))
                db.execute('COMMIT')
                return attempt
        except sqlite3.DatabaseError:
            raise LifecycleError('NATIVE_JOURNAL_CORRUPT_OR_BUSY') from None

    def record_observation(self, phase: str, attempt_id: str, observation: Observation,
                           context: RunContext) -> None:
        observation.validate(context)
        _require(observation.phase == phase, 'OBSERVATION_PHASE_MISMATCH')
        raw = observation.to_bytes()
        _require(len(raw) <= MAX_REFERENCE_BYTES, 'EVIDENCE_REFERENCE_SIZE_INVALID')
        try:
            with self._db() as db:
                db.execute('BEGIN IMMEDIATE')
                current = self._context(db)
                _require(current == context, 'RUN_CONTEXT_MISMATCH')
                row = db.execute('SELECT phase, status, attempt_id FROM checkpoint ORDER BY position DESC LIMIT 1').fetchone()
                _require(row is not None and row[0] == phase and row[1] == 'OBSERVATION_STARTED' and row[2] == attempt_id,
                         'OBSERVATION_ATTEMPT_MISMATCH')
                db.execute('UPDATE checkpoint SET status=?, observation=?, observation_sha256=?, updated_at_utc=? WHERE phase=?',
                           ('OBSERVATION_PENDING_VERIFICATION', raw, hashlib.sha256(raw).hexdigest(), _clock(), phase))
                db.execute('COMMIT')
        except sqlite3.DatabaseError:
            raise LifecycleError('NATIVE_JOURNAL_CORRUPT_OR_BUSY') from None

    def record_verification(self, phase: str, attempt_id: str, result: VerificationResult) -> None:
        result.validate()
        raw = json.dumps(asdict(result), sort_keys=True, separators=(',', ':')).encode('utf-8')
        try:
            with self._db() as db:
                db.execute('BEGIN IMMEDIATE')
                context = self._context(db)
                result.validate(context)
                row = db.execute('SELECT phase, status, attempt_id FROM checkpoint ORDER BY position DESC LIMIT 1').fetchone()
                _require(row is not None and row[0] == phase and row[1] == 'OBSERVATION_PENDING_VERIFICATION' and row[2] == attempt_id,
                         'VERIFICATION_ATTEMPT_MISMATCH')
                state = 'VERIFIED_PENDING_AUTHORITY' if result.verified else 'BLOCKED_VERIFICATION_FAILED'
                db.execute('UPDATE checkpoint SET status=?, verification=?, verification_sha256=?, updated_at_utc=? WHERE phase=?',
                           (state, raw, hashlib.sha256(raw).hexdigest(), _clock(), phase))
                db.execute('COMMIT')
        except sqlite3.DatabaseError:
            raise LifecycleError('NATIVE_JOURNAL_CORRUPT_OR_BUSY') from None

    def recover_interruption(self) -> dict:
        state = self.status()
        pending = state['pending']
        if pending is None:
            return {'recovery': 'READY', 'automatic_replay': False}
        phase, status = pending['phase'], pending['checkpoint_state']
        # Mutating operations are never replayed. Even read-only observations
        # require a fresh independent inspection; the adapter does not rerun them.
        return {'recovery': 'MANUAL_RECONCILIATION_REQUIRED', 'phase': phase,
                'checkpoint_state': status, 'automatic_replay': False,
                'authority': 'BLOCKED'}


class NativeLifecycleCoordinator:
    """Separates collection, independent re-query, and trusted phase advancement."""

    def __init__(self, journal: NativePendingJournal, observer: Observer,
                 verifier: IndependentVerifier):
        self.journal, self.observer, self.verifier = journal, observer, verifier
        self.authority: TrustedPhaseAdvancer = NativeAuthorityUnavailable()

    def observe_next(self, phase: str, context: RunContext) -> dict:
        attempt = self.journal.begin_observation(phase)
        observation = self.observer.observe(phase, context)
        observation.validate(context)
        self.journal.record_observation(phase, attempt, observation, context)
        verification = self.verifier.verify(phase, context, observation)
        verification.validate(context)
        self.journal.record_verification(phase, attempt, verification)
        if not verification.verified:
            return {'phase': phase, 'status': 'BLOCKED_VERIFICATION_FAILED',
                    'reason': verification.reason, 'native_authority': 'BLOCKED'}
        try:
            self.authority.advance(phase, context, observation, verification)
        except LifecycleError as exc:
            if str(exc) != 'P028_NATIVE_AUTHORITY_UNAVAILABLE':
                raise
            return {'phase': phase, 'status': 'VERIFIED_PENDING_AUTHORITY',
                    'native_authority': 'BLOCKED', 'reason': str(exc)}
        return {'phase': phase, 'status': 'AUTHORIZED'}
