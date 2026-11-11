#!/usr/bin/env python3
"""Transactional P028 coordinator core for disposable fixtures only.

This module is not a Windows trust authority and never starts product or OS
mutation scripts. Native operation stays blocked until an independently
verified Windows ACL and observer adapter exists.
"""
from __future__ import annotations

import argparse
from contextlib import contextmanager
import datetime as dt
import hashlib
import json
import os
import secrets
import sqlite3
import sys
from pathlib import Path

PHASES = ('PREFLIGHT', 'CANDIDATE_VERIFIED', 'ENVIRONMENT_QUALIFIED', 'INSTALLED',
          'GUI_VERIFIED', 'IPC_VERIFIED', 'STARTUP_CONFIGURED', 'LOGOFF_OBSERVED',
          'NEW_LOGON_OBSERVED', 'LOGON_TRIGGER_VERIFIED', 'LIFECYCLE_VERIFIED',
          'CLEANUP_VERIFIED', 'EVIDENCE_FINALIZED')
MAX_EVIDENCE = 4 * 1024 * 1024


class CoordinatorError(ValueError):
    pass


def require(condition: bool, reason: str) -> None:
    if not condition:
        raise CoordinatorError(reason)


def clock() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat(timespec='seconds')


def open_db(path: Path) -> sqlite3.Connection:
    require(path.is_file() and not path.is_symlink(), 'STATE_UNSAFE_OR_MISSING')
    db = sqlite3.connect(str(path), timeout=5, isolation_level=None)
    try:
        db.execute('PRAGMA foreign_keys=ON')
        db.execute('PRAGMA synchronous=FULL')
    except BaseException:
        db.close()
        raise
    return db


@contextmanager
def database(path: Path):
    """Close every connection deterministically; closing rolls back open transactions."""
    db = open_db(path)
    try:
        yield db
    except BaseException:
        if db.in_transaction:
            db.rollback()
        raise
    else:
        if db.in_transaction:
            db.rollback()
    finally:
        db.close()


def initialize(path: Path, *, fixture: bool, approval: str, expires: str) -> str:
    require(fixture is True, 'NATIVE_AUTHORITY_UNAVAILABLE')
    require(approval == 'I_APPROVE_DISPOSABLE_FIXTURE_ONLY', 'FIXTURE_APPROVAL_REQUIRED')
    require(path.name == 'p028-fixture-state.sqlite3' and path.parent.is_dir() and not path.parent.is_symlink(), 'STATE_PATH_UNSAFE')
    try:
        deadline = dt.datetime.fromisoformat(expires)
        now = dt.datetime.now(dt.timezone.utc)
        require(deadline.tzinfo is not None and now < deadline <= now + dt.timedelta(days=7), 'DEADLINE_INVALID')
    except ValueError:
        raise CoordinatorError('DEADLINE_INVALID') from None
    try:
        fd = os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
    except FileExistsError:
        raise CoordinatorError('STATE_ALREADY_EXISTS') from None
    os.close(fd)
    run_id = secrets.token_hex(16)
    try:
        with database(path) as db:
            db.execute('BEGIN IMMEDIATE')
            db.execute('CREATE TABLE run (id TEXT PRIMARY KEY, mode TEXT NOT NULL, created TEXT NOT NULL, deadline TEXT NOT NULL, status TEXT NOT NULL)')
            db.execute('CREATE TABLE phases (position INTEGER PRIMARY KEY, name TEXT UNIQUE NOT NULL, digest TEXT UNIQUE NOT NULL, evidence BLOB NOT NULL, completed TEXT NOT NULL)')
            db.execute('CREATE TABLE cleanup (id INTEGER PRIMARY KEY CHECK (id=1), result TEXT NOT NULL, digest TEXT NOT NULL, evidence BLOB NOT NULL, recorded TEXT NOT NULL)')
            db.execute('INSERT INTO run VALUES (?, ?, ?, ?, ?)', (run_id, 'FIXTURE_ONLY', clock(), deadline.isoformat(), 'ACTIVE'))
            db.execute('COMMIT')
    except Exception:
        # Retain a failed initialization for inspection; never delete unknown state.
        raise
    return run_id


def _run(db: sqlite3.Connection) -> tuple:
    try:
        rows = db.execute('SELECT id, mode, deadline, status FROM run').fetchall()
    except sqlite3.DatabaseError:
        raise CoordinatorError('STATE_CORRUPT') from None
    require(len(rows) == 1 and rows[0][1] == 'FIXTURE_ONLY', 'STATE_CORRUPT')
    return rows[0]


def read_state(path: Path) -> dict:
    try:
        with database(path) as db:
            run_id, mode, deadline, status = _run(db)
            rows = db.execute('SELECT position, name, digest, evidence FROM phases ORDER BY position').fetchall()
            for index, (position, name, digest, evidence) in enumerate(rows):
                require(position == index and index < len(PHASES) and name == PHASES[index], 'JOURNAL_CORRUPT')
                require(hashlib.sha256(evidence).hexdigest() == digest, 'EVIDENCE_CORRUPT')
            cleanup = db.execute('SELECT result, digest, evidence FROM cleanup').fetchall()
            for _, digest, evidence in cleanup:
                require(hashlib.sha256(evidence).hexdigest() == digest, 'CLEANUP_CORRUPT')
            return {'run_id': run_id, 'mode': mode, 'deadline': deadline, 'status': status,
                    'completed': [r[1] for r in rows], 'cleanup': cleanup[0][0] if cleanup else 'NOT_RUN',
                    'native_result': 'BLOCKED_UNVERIFIED_NATIVE_PROVENANCE'}
    except sqlite3.DatabaseError:
        raise CoordinatorError('STATE_CORRUPT') from None


def advance(path: Path, phase: str, evidence: bytes) -> None:
    require(isinstance(evidence, bytes) and 0 < len(evidence) <= MAX_EVIDENCE, 'EVIDENCE_SIZE_INVALID')
    try:
        item = json.loads(evidence)
    except (ValueError, UnicodeDecodeError):
        raise CoordinatorError('EVIDENCE_INVALID') from None
    require(isinstance(item, dict) and set(item) == {'run_id', 'phase', 'result', 'mode'}, 'EVIDENCE_INVALID')
    require(item['mode'] == 'FIXTURE_ONLY' and item['result'] == 'PASS', 'EVIDENCE_NOT_FIXTURE_PASS')
    try:
        with database(path) as db:
            db.execute('BEGIN IMMEDIATE')
            run_id, _, deadline, status = _run(db)
            require(status == 'ACTIVE', 'RUN_TERMINAL')
            require(dt.datetime.now(dt.timezone.utc) <= dt.datetime.fromisoformat(deadline), 'RUN_EXPIRED')
            require(item['run_id'] == run_id, 'RUN_MISMATCH')
            rows = db.execute('SELECT position, name, digest, evidence FROM phases ORDER BY position').fetchall()
            for index, (position, name, digest, raw) in enumerate(rows):
                require(position == index and index < len(PHASES) and name == PHASES[index], 'JOURNAL_CORRUPT')
                require(hashlib.sha256(raw).hexdigest() == digest, 'EVIDENCE_CORRUPT')
            require(phase not in [r[1] for r in rows], 'PHASE_DUPLICATE')
            require(len(rows) < len(PHASES) and phase == PHASES[len(rows)] and item['phase'] == phase, 'PHASE_ORDER_INVALID')
            if phase == 'EVIDENCE_FINALIZED':
                cleanup = db.execute('SELECT result FROM cleanup WHERE id=1').fetchone()
                require(cleanup is not None and cleanup[0] == 'PASS', 'CLEANUP_NOT_VERIFIED')
            digest = hashlib.sha256(evidence).hexdigest()
            require(not db.execute('SELECT 1 FROM phases WHERE digest=?', (digest,)).fetchone(), 'EVIDENCE_REPLAY')
            db.execute('INSERT INTO phases VALUES (?, ?, ?, ?, ?)', (len(rows), phase, digest, evidence, clock()))
            if phase == 'EVIDENCE_FINALIZED':
                db.execute("UPDATE run SET status='FINALIZED'")
            db.execute('COMMIT')
    except sqlite3.DatabaseError:
        raise CoordinatorError('STATE_CORRUPT_OR_BUSY') from None


def record_cleanup(path: Path, evidence: bytes) -> None:
    require(isinstance(evidence, bytes) and 0 < len(evidence) <= MAX_EVIDENCE, 'EVIDENCE_SIZE_INVALID')
    try:
        item = json.loads(evidence)
    except (ValueError, UnicodeDecodeError):
        raise CoordinatorError('EVIDENCE_INVALID') from None
    require(isinstance(item, dict) and set(item) == {'run_id', 'mode', 'result'}, 'EVIDENCE_INVALID')
    require(item['mode'] == 'FIXTURE_ONLY' and item['result'] in ('PASS', 'FAIL', 'BLOCKED'), 'CLEANUP_RESULT_INVALID')
    try:
        with database(path) as db:
            db.execute('BEGIN IMMEDIATE')
            run_id, _, _, status = _run(db)
            require(status == 'ACTIVE', 'RUN_TERMINAL')
            require(item['run_id'] == run_id, 'RUN_MISMATCH')
            require(db.execute('SELECT 1 FROM cleanup').fetchone() is None, 'CLEANUP_DUPLICATE')
            db.execute('INSERT INTO cleanup VALUES (1, ?, ?, ?, ?)', (item['result'], hashlib.sha256(evidence).hexdigest(), evidence, clock()))
            db.execute('COMMIT')
    except sqlite3.DatabaseError:
        raise CoordinatorError('STATE_CORRUPT_OR_BUSY') from None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--state', required=True, type=Path)
    sub = parser.add_subparsers(dest='command', required=True)
    init = sub.add_parser('init-fixture')
    init.add_argument('--approval', required=True)
    init.add_argument('--expires', required=True)
    step = sub.add_parser('advance-fixture')
    step.add_argument('phase', choices=PHASES)
    step.add_argument('evidence', type=Path)
    cleanup = sub.add_parser('record-fixture-cleanup')
    cleanup.add_argument('evidence', type=Path)
    sub.add_parser('status')
    args = parser.parse_args()
    try:
        if args.command == 'init-fixture':
            print(initialize(args.state, fixture=True, approval=args.approval, expires=args.expires))
        elif args.command == 'status':
            print(json.dumps(read_state(args.state), sort_keys=True))
        else:
            require(args.evidence.is_file() and not args.evidence.is_symlink() and args.evidence.stat().st_size <= MAX_EVIDENCE, 'EVIDENCE_UNSAFE')
            raw = args.evidence.read_bytes()
            if args.command == 'advance-fixture':
                advance(args.state, args.phase, raw)
            else:
                record_cleanup(args.state, raw)
            print('FIXTURE_ONLY: native acceptance remains blocked')
    except (CoordinatorError, OSError, sqlite3.DatabaseError) as exc:
        print(f'P028_COORDINATOR: {exc}', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
