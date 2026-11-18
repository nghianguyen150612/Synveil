"""Adversarial tests for synthetic P028 provenance fixtures only."""
from __future__ import annotations

import datetime as dt
import unittest

import windows_native_provenance as provenance


UTC = dt.timezone.utc
NOW = dt.datetime(2026, 11, 17, 12, 0, 0, tzinfo=UTC)


def stamp(offset: int) -> str:
    return (NOW + dt.timedelta(seconds=offset)).strftime("%Y-%m-%dT%H:%M:%SZ")


def context() -> dict:
    return {
        "machine_guid_sha256": "a" * 64,
        "boot_time_sha256": "7" * 64,
        "subject_sid_sha256": "b" * 64,
        "observer_sid_sha256": "c" * 64,
        "controller_sid_sha256": "d" * 64,
        "candidate_sha256": "e" * 64,
        "manifest_sha256": "f" * 64,
        "source_commit": "1" * 40,
        "harness_commit": "2" * 40,
        "run_created_at_utc": stamp(-500),
        "run_expires_at_utc": stamp(500),
        "expected_task_name": r"\Synveil\BackgroundClient\profile-test",
    }


def event(channel: str, event_id: int, record_id: int, occurred: int, *, machine: str = "a" * 64) -> dict:
    provider = ("Microsoft-Windows-Security-Auditing" if channel == "Security"
                else "Microsoft-Windows-TaskScheduler")
    return {
        "channel": channel,
        "provider": provider,
        "event_id": event_id,
        "record_id": record_id,
        "machine_guid_sha256": machine,
        "boot_time_sha256": "7" * 64,
        "occurred_at_utc": stamp(occurred),
        "record_sha256": "9" * 64,
    }


def session(luid: str, session_id: int = 3) -> dict:
    return {"user_sid_sha256": "b" * 64, "authentication_luid_sha256": luid,
            "windows_session_id": session_id}


def record(event_value: dict, *, session_value: dict, observed: int,
           logon_type: int | None = None, mode: str = "FIXTURE_ONLY") -> dict:
    result = {"schema_version": 1, "mode": mode, "result": "PASS", "run_id": "3" * 32,
              "context": context(), "event": event_value, "session": session_value,
              "observed_at_utc": stamp(observed)}
    if logon_type is not None:
        result["logon_type"] = logon_type
    return result


def valid_sequence() -> dict:
    initial = record(event("Security", 4624, 100, -300), session_value=session("4" * 64),
                     observed=-290, logon_type=2)
    logoff = record(event("Security", 4634, 101, -100), session_value=session("4" * 64),
                    observed=-90)
    new = record(event("Security", 4624, 102, -50), session_value=session("5" * 64, 5),
                 observed=-40, logon_type=10)
    trigger = record(event("Microsoft-Windows-TaskScheduler/Operational", 119, 103, -30),
                     session_value=session("5" * 64, 5), observed=-20)
    trigger.pop("session")
    trigger["task_name"] = context()["expected_task_name"]
    trigger["trigger_kind"] = "LOGON"
    trigger["manual_task_run_present"] = False
    trigger["task_process"] = session("5" * 64, 5)
    return {"initial_session": initial, "logoff": logoff, "new_logon": new,
            "logon_trigger": trigger}


class NativeProvenanceFixtureTests(unittest.TestCase):
    def validate(self, records: dict | None = None) -> dict:
        return provenance.validate_fixture_sequence(records or valid_sequence(), now=stamp(0))

    def assertRejected(self, code: str, records: dict | None = None) -> None:
        with self.assertRaisesRegex(provenance.ProvenanceError, code):
            self.validate(records)

    def test_ordered_identity_bound_fixture_stays_non_authoritative(self) -> None:
        self.assertEqual(self.validate(), {"mode": "FIXTURE_ONLY",
                                          "native_authority": "BLOCKED_UNAUTHENTICATED_FIXTURE"})

    def test_wrong_run_or_machine_is_rejected(self) -> None:
        data = valid_sequence()
        data["new_logon"]["run_id"] = "4" * 32
        self.assertRejected("CROSS_RUN_OR_CONTEXT_SUBSTITUTION", data)
        data = valid_sequence()
        data["new_logon"]["event"]["machine_guid_sha256"] = "0" * 64
        self.assertRejected("EVENT_MACHINE_MISMATCH", data)
        data = valid_sequence()
        data["new_logon"]["event"]["boot_time_sha256"] = "0" * 64
        self.assertRejected("EVENT_BOOT_MISMATCH", data)

    def test_wrong_subject_observer_or_controller_is_rejected(self) -> None:
        data = valid_sequence()
        data["new_logon"]["session"]["user_sid_sha256"] = "0" * 64
        self.assertRejected("SESSION_SUBJECT_MISMATCH", data)
        data = valid_sequence()
        data["initial_session"]["context"]["observer_sid_sha256"] = "b" * 64
        self.assertRejected("OBSERVER_SUBJECT_IDENTITY_COLLISION", data)
        data = valid_sequence()
        data["initial_session"]["context"]["controller_sid_sha256"] = "b" * 64
        self.assertRejected("CONTROLLER_SUBJECT_IDENTITY_COLLISION", data)
        data = valid_sequence()
        data["new_logon"]["context"]["observer_sid_sha256"] = "8" * 64
        self.assertRejected("CROSS_RUN_OR_CONTEXT_SUBSTITUTION", data)

    def test_replay_duplicate_event_and_reused_luid_are_rejected(self) -> None:
        data = valid_sequence()
        data["new_logon"]["event"]["record_id"] = data["logoff"]["event"]["record_id"]
        self.assertRejected("EVENT_RECORD_REPLAY", data)
        data = valid_sequence()
        data["new_logon"]["session"]["authentication_luid_sha256"] = "4" * 64
        data["logon_trigger"]["task_process"]["authentication_luid_sha256"] = "4" * 64
        self.assertRejected("AUTHENTICATION_SESSION_REUSED", data)

    def test_wrong_phase_order_and_logoff_session_are_rejected(self) -> None:
        data = valid_sequence()
        data["new_logon"]["event"]["occurred_at_utc"] = stamp(-101)
        self.assertRejected("LOGON_LOGOFF_ORDER_INVALID", data)
        data = valid_sequence()
        data["logoff"]["session"]["windows_session_id"] = 99
        self.assertRejected("LOGOFF_WINDOWS_SESSION_MISMATCH", data)

    def test_missing_malformed_or_unsupported_event_fields_block(self) -> None:
        data = valid_sequence()
        data["new_logon"]["event"].pop("record_id")
        self.assertRejected("EVENT_RECORD_ID_INVALID", data)
        data = valid_sequence()
        data["new_logon"]["event"]["event_id"] = 999
        self.assertRejected("EVENT_SOURCE_OR_ID_UNSUPPORTED", data)
        data = valid_sequence()
        data["new_logon"]["observed_at_utc"] = "yesterday"
        self.assertRejected("OBSERVATION_TIME_INVALID", data)
        data = valid_sequence()
        data["initial_session"].pop("session")
        self.assertRejected("SESSION_BINDING_MISSING", data)

    def test_partial_sequence_is_not_resumable_as_native_authority(self) -> None:
        data = valid_sequence()
        del data["new_logon"]
        self.assertRejected("SEQUENCE_INVALID", data)

    def test_stale_observation_and_inconsistent_time_are_rejected(self) -> None:
        data = valid_sequence()
        data["initial_session"]["event"]["occurred_at_utc"] = stamp(-400)
        data["initial_session"]["observed_at_utc"] = stamp(-390)
        self.assertRejected("OBSERVATION_STALE", data)
        data = valid_sequence()
        data["logoff"]["event"]["occurred_at_utc"] = stamp(-300)
        data["logoff"]["observed_at_utc"] = stamp(-310)
        self.assertRejected("EVENT_TIMELINE_INVALID", data)

    def test_manual_or_wrong_session_task_activity_is_rejected(self) -> None:
        data = valid_sequence()
        data["logon_trigger"]["manual_task_run_present"] = True
        self.assertRejected("MANUAL_TASK_RUN_REJECTED", data)
        data = valid_sequence()
        data["logon_trigger"]["task_process"]["authentication_luid_sha256"] = "8" * 64
        self.assertRejected("TASK_PROCESS_SESSION_MISMATCH", data)

    def test_timed_and_user_initiated_events_cannot_claim_logon_trigger(self) -> None:
        for event_id in (107, 110):
            with self.subTest(event_id=event_id):
                data = valid_sequence()
                data["logon_trigger"]["event"]["event_id"] = event_id
                self.assertRejected("EVENT_SOURCE_OR_ID_UNSUPPORTED", data)

    def test_same_admin_sid_cannot_fill_both_roles(self) -> None:
        data = valid_sequence()
        data["initial_session"]["context"]["observer_sid_sha256"] = "d" * 64
        self.assertRejected("CONTROLLER_OBSERVER_IDENTITY_COLLISION", data)

    def test_untrusted_json_cannot_be_promoted_to_native_evidence(self) -> None:
        data = valid_sequence()
        data["new_logon"]["mode"] = "NATIVE"
        self.assertRejected("NATIVE_EVIDENCE_NOT_AUTHENTICATED", data)


if __name__ == "__main__":
    unittest.main()
