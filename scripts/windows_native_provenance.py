"""Fixture-only model for P028 authenticated Windows event correlation.

This module validates the shape and ordering expected from the native observer.
It does not query Windows, authenticate JSON, or authorize native coordinator
transitions. Synthetic records are never proof of a Windows interactive session.
"""
from __future__ import annotations

import datetime as dt
import re
from typing import Any


class ProvenanceError(ValueError):
    """A malformed, inconsistent, stale, or replayed fixture observation."""


_HEX64 = re.compile(r"^[0-9a-f]{64}$")
_RUN = re.compile(r"^[0-9a-f]{32}$")
_UTC_FORMAT = "%Y-%m-%dT%H:%M:%SZ"
_MAX_OBSERVATION_AGE = dt.timedelta(minutes=5)


def _require(condition: bool, code: str) -> None:
    if not condition:
        raise ProvenanceError(code)


def _utc(value: Any, code: str) -> dt.datetime:
    _require(isinstance(value, str) and value.endswith("Z"), code)
    try:
        result = dt.datetime.strptime(value, _UTC_FORMAT).replace(tzinfo=dt.timezone.utc)
    except ValueError:
        raise ProvenanceError(code) from None
    return result


def _digest(value: Any, code: str) -> None:
    _require(isinstance(value, str) and _HEX64.fullmatch(value) is not None, code)


def _validate_context(record: Any, now: dt.datetime) -> dict[str, Any]:
    _require(isinstance(record, dict), "EVIDENCE_SCHEMA_INVALID")
    _require(record.get("schema_version") == 1, "EVIDENCE_SCHEMA_UNSUPPORTED")
    _require(record.get("mode") == "FIXTURE_ONLY" and record.get("result") == "PASS",
             "NATIVE_EVIDENCE_NOT_AUTHENTICATED")
    _require(isinstance(record.get("run_id"), str) and _RUN.fullmatch(record["run_id"]) is not None,
             "RUN_ID_INVALID")
    context = record.get("context")
    _require(isinstance(context, dict), "CONTEXT_INVALID")
    for key in ("machine_guid_sha256", "boot_time_sha256", "subject_sid_sha256", "observer_sid_sha256",
                "controller_sid_sha256", "candidate_sha256", "manifest_sha256"):
        _digest(context.get(key), "CONTEXT_DIGEST_INVALID")
    _require(context["observer_sid_sha256"] != context["subject_sid_sha256"],
             "OBSERVER_SUBJECT_IDENTITY_COLLISION")
    _require(context["controller_sid_sha256"] != context["subject_sid_sha256"],
             "CONTROLLER_SUBJECT_IDENTITY_COLLISION")
    _require(context["controller_sid_sha256"] != context["observer_sid_sha256"],
             "CONTROLLER_OBSERVER_IDENTITY_COLLISION")
    for key, pattern in (("source_commit", re.compile(r"^[0-9a-f]{40}$")),
                         ("harness_commit", re.compile(r"^[0-9a-f]{40}$"))):
        _require(isinstance(context.get(key), str) and pattern.fullmatch(context[key]) is not None,
                 "CONTEXT_COMMIT_INVALID")
    created = _utc(context.get("run_created_at_utc"), "RUN_TIME_INVALID")
    expires = _utc(context.get("run_expires_at_utc"), "RUN_TIME_INVALID")
    _require(created < expires and expires - created <= dt.timedelta(days=7), "RUN_TIME_INVALID")
    _require(created <= now <= expires, "RUN_EXPIRED_OR_NOT_STARTED")
    return context


def _validate_event(record: Any, event_id: int, channel: str, now: dt.datetime,
                    context: dict[str, Any]) -> tuple[dt.datetime, tuple[str, str, int]]:
    _require(isinstance(record, dict), "EVENT_RECORD_INVALID")
    event = record.get("event")
    _require(isinstance(event, dict), "EVENT_RECORD_INVALID")
    _require(event.get("channel") == channel and event.get("event_id") == event_id,
             "EVENT_SOURCE_OR_ID_UNSUPPORTED")
    provider = "Microsoft-Windows-Security-Auditing" if channel == "Security" else "Microsoft-Windows-TaskScheduler"
    _require(event.get("provider") == provider, "EVENT_PROVIDER_UNSUPPORTED")
    _require(type(event.get("record_id")) is int and event["record_id"] > 0, "EVENT_RECORD_ID_INVALID")
    _require(event.get("machine_guid_sha256") == context["machine_guid_sha256"], "EVENT_MACHINE_MISMATCH")
    _require(event.get("boot_time_sha256") == context["boot_time_sha256"], "EVENT_BOOT_MISMATCH")
    _digest(event.get("record_sha256"), "EVENT_DIGEST_INVALID")
    occurred = _utc(event.get("occurred_at_utc"), "EVENT_TIME_INVALID")
    observed = _utc(record.get("observed_at_utc"), "OBSERVATION_TIME_INVALID")
    created = _utc(context["run_created_at_utc"], "RUN_TIME_INVALID")
    expires = _utc(context["run_expires_at_utc"], "RUN_TIME_INVALID")
    _require(created <= occurred <= observed <= now <= expires, "EVENT_TIMELINE_INVALID")
    _require(now - observed <= _MAX_OBSERVATION_AGE, "OBSERVATION_STALE")
    return occurred, (channel, provider, event["record_id"])


def validate_fixture_sequence(records: dict[str, Any], *, now: str) -> dict[str, str]:
    """Check a synthetic sequence against the observer contract.

    The return value is deliberately `FIXTURE_ONLY`; it cannot be consumed by
    `protect-windows-native-run-state.ps1` or converted into native acceptance.
    """
    instant = _utc(now, "NOW_INVALID")
    _require(isinstance(records, dict) and set(records) ==
             {"initial_session", "logoff", "new_logon", "logon_trigger"}, "SEQUENCE_INVALID")
    context: dict[str, Any] | None = None
    for value in records.values():
        current = _validate_context(value, instant)
        if context is None:
            context = current
        else:
            _require(current == context and value.get("run_id") == next(iter(records.values())).get("run_id"),
                     "CROSS_RUN_OR_CONTEXT_SUBSTITUTION")
    assert context is not None

    initial, logoff, new_logon, trigger = (records[name] for name in
        ("initial_session", "logoff", "new_logon", "logon_trigger"))
    initial_time, initial_key = _validate_event(initial, 4624, "Security", instant, context)
    logoff_time, logoff_key = _validate_event(logoff, 4634, "Security", instant, context)
    new_time, new_key = _validate_event(new_logon, 4624, "Security", instant, context)
    trigger_time, trigger_key = _validate_event(
        trigger, 119, "Microsoft-Windows-TaskScheduler/Operational", instant, context
    )
    keys = (initial_key, logoff_key, new_key, trigger_key)
    _require(len(set(keys)) == len(keys), "EVENT_RECORD_REPLAY")

    subject = context["subject_sid_sha256"]
    initial_session = initial.get("session")
    old_session = logoff.get("session")
    new_session = new_logon.get("session")
    trigger_session = trigger.get("task_process")
    sessions = (initial_session, old_session, new_session, trigger_session)
    for session in sessions:
        _require(isinstance(session, dict), "SESSION_BINDING_MISSING")
        _require(session.get("user_sid_sha256") == subject, "SESSION_SUBJECT_MISMATCH")
        _digest(session.get("authentication_luid_sha256"), "SESSION_LUID_INVALID")
        _require(type(session.get("windows_session_id")) is int and session["windows_session_id"] > 0,
                 "WINDOWS_SESSION_ID_INVALID")
    _require(initial.get("logon_type") in (2, 10), "INITIAL_LOGON_TYPE_UNSUPPORTED")
    _require(new_logon.get("logon_type") in (2, 10), "NEW_LOGON_TYPE_UNSUPPORTED")
    _require(old_session["authentication_luid_sha256"] == initial_session["authentication_luid_sha256"],
             "LOGOFF_SESSION_MISMATCH")
    _require(old_session["windows_session_id"] == initial_session["windows_session_id"],
             "LOGOFF_WINDOWS_SESSION_MISMATCH")
    _require(new_session["authentication_luid_sha256"] != initial_session["authentication_luid_sha256"],
             "AUTHENTICATION_SESSION_REUSED")
    _require(logoff_time > initial_time and new_time > logoff_time and trigger_time >= new_time,
             "LOGON_LOGOFF_ORDER_INVALID")
    _require(trigger.get("task_name") == context.get("expected_task_name") and
             trigger.get("trigger_kind") == "LOGON", "LOGON_TRIGGER_NOT_PROVEN")
    _require(trigger.get("manual_task_run_present") is False, "MANUAL_TASK_RUN_REJECTED")
    _require(trigger_session["authentication_luid_sha256"] == new_session["authentication_luid_sha256"] and
             trigger_session["windows_session_id"] == new_session["windows_session_id"],
             "TASK_PROCESS_SESSION_MISMATCH")
    return {"mode": "FIXTURE_ONLY", "native_authority": "BLOCKED_UNAUTHENTICATED_FIXTURE"}
