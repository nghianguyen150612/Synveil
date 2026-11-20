#!/usr/bin/env python3
"""Audit an offline P028 phase ledger. Integrity is not native provenance.

This read-only verifier never authorizes execution or declares native acceptance.
The ledger and evidence must be retained under an independently protected root.
"""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import re
import sys
from pathlib import Path

PHASES = (
    'PREFLIGHT', 'CANDIDATE_VERIFIED', 'ENVIRONMENT_QUALIFIED', 'INSTALLED',
    'GUI_VERIFIED', 'IPC_VERIFIED', 'STARTUP_CONFIGURED', 'LOGOFF_OBSERVED',
    'NEW_LOGON_OBSERVED', 'LOGON_TRIGGER_VERIFIED', 'LIFECYCLE_VERIFIED',
    'CLEANUP_VERIFIED', 'EVIDENCE_FINALIZED',
)
HEX64 = re.compile(r'^[0-9a-f]{64}$')
HEX40 = re.compile(r'^[0-9a-f]{40}$')
RUN = re.compile(r'^[0-9a-f]{32}$')
MAX_BYTES = 4 * 1024 * 1024


class LedgerError(ValueError):
    pass


def require(condition: bool, reason: str) -> None:
    if not condition:
        raise LedgerError(reason)


def read_bounded(path: Path) -> bytes:
    require(path.is_file() and not path.is_symlink(), 'EVIDENCE_MISSING_OR_UNSAFE')
    require(path.stat().st_size <= MAX_BYTES, 'EVIDENCE_TOO_LARGE')
    return path.read_bytes()


def timestamp(value: object) -> dt.datetime:
    try:
        require(isinstance(value, str) and value.endswith('Z'), 'TIMESTAMP_INVALID')
        parsed = dt.datetime.fromisoformat(value[:-1] + '+00:00')
        require(parsed.utcoffset() == dt.timedelta(0), 'TIMESTAMP_INVALID')
        return parsed
    except (ValueError, OverflowError):
        raise LedgerError('TIMESTAMP_INVALID') from None


def verify(ledger: object, evidence_root: Path) -> str:
    require(isinstance(ledger, dict) and set(ledger) == {'schema_version', 'context', 'phases'}, 'LEDGER_SHAPE')
    require(type(ledger['schema_version']) is int and ledger['schema_version'] == 1, 'SCHEMA_UNSUPPORTED')
    context = ledger['context']
    keys = {'run_id', 'source_commit', 'harness_commit', 'candidate_sha256', 'manifest_sha256',
            'producer_run_id', 'machine_sha256', 'user_sid_sha256', 'windows_build', 'architecture',
            'started_at_utc', 'expires_at_utc'}
    require(isinstance(context, dict) and set(context) == keys, 'CONTEXT_SHAPE')
    require(isinstance(context['run_id'], str) and RUN.fullmatch(context['run_id']) is not None, 'RUN_ID_INVALID')
    for key in ('source_commit', 'harness_commit'):
        require(isinstance(context[key], str) and HEX40.fullmatch(context[key]) is not None, 'COMMIT_INVALID')
    for key in ('candidate_sha256', 'manifest_sha256', 'machine_sha256', 'user_sid_sha256'):
        require(isinstance(context[key], str) and HEX64.fullmatch(context[key]) is not None, 'CONTEXT_DIGEST_INVALID')
    require(isinstance(context['producer_run_id'], str) and re.fullmatch(r'[0-9]{1,32}', context['producer_run_id']) is not None, 'PRODUCER_INVALID')
    require(type(context['windows_build']) is int and context['windows_build'] >= 22000 and context['architecture'] == 'x86_64', 'PLATFORM_UNSUPPORTED')
    start, end = timestamp(context['started_at_utc']), timestamp(context['expires_at_utc'])
    require(start < end and end - start <= dt.timedelta(days=7), 'RUN_WINDOW_INVALID')
    phases = ledger['phases']
    require(isinstance(phases, list) and len(phases) <= len(PHASES), 'PHASE_COUNT_INVALID')
    root = evidence_root.resolve(strict=True)
    require(root.is_dir(), 'EVIDENCE_ROOT_INVALID')
    previous_time = start
    seen: set[str] = set()
    for index, phase in enumerate(phases):
        require(isinstance(phase, dict) and set(phase) == {'phase', 'run_id', 'completed_at_utc', 'file', 'sha256'}, 'PHASE_SHAPE')
        require(phase['phase'] == PHASES[index], 'PHASE_ORDER_INVALID')
        require(phase['run_id'] == context['run_id'], 'RUN_MISMATCH')
        instant = timestamp(phase['completed_at_utc'])
        require(previous_time <= instant <= end, 'PHASE_TIME_INVALID')
        previous_time = instant
        name = phase['file']
        require(isinstance(name, str) and re.fullmatch(r'[A-Za-z0-9_-]{1,80}\.json', name) is not None, 'EVIDENCE_PATH_INVALID')
        require(name not in seen, 'EVIDENCE_REUSED')
        seen.add(name)
        digest = phase['sha256']
        require(isinstance(digest, str) and HEX64.fullmatch(digest) is not None, 'EVIDENCE_DIGEST_INVALID')
        raw = read_bounded(root / name)
        require(hashlib.sha256(raw).hexdigest() == digest, 'EVIDENCE_DIGEST_MISMATCH')
        try:
            item = json.loads(raw)
        except (ValueError, UnicodeDecodeError):
            raise LedgerError('EVIDENCE_JSON_INVALID') from None
        require(isinstance(item, dict), 'EVIDENCE_SHAPE')
        require(item.get('schema_version') == 1 and item.get('phase') == phase['phase'], 'EVIDENCE_PHASE_MISMATCH')
        require(item.get('context') == context, 'EVIDENCE_CONTEXT_MISMATCH')
        require(item.get('result') == 'PASS', 'PHASE_NOT_PASS')
        require(item.get('completed_at_utc') == phase['completed_at_utc'], 'EVIDENCE_TIME_MISMATCH')
    # A hash chain maintained by a caller is not an authenticated observer.
    return 'BLOCKED_UNVERIFIED_NATIVE_PROVENANCE'


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('ledger', type=Path)
    parser.add_argument('evidence_root', type=Path)
    args = parser.parse_args()
    try:
        ledger = json.loads(read_bounded(args.ledger))
        result = verify(ledger, args.evidence_root)
    except (LedgerError, OSError, ValueError, TypeError) as exc:
        print(f'P028_LEDGER_INVALID: {exc}', file=sys.stderr)
        return 1
    print(result)
    return 2


if __name__ == '__main__':
    raise SystemExit(main())
