#!/usr/bin/env python3
"""Fail-closed validator for qualified P028/P013 Windows evidence v1."""

from __future__ import annotations

import argparse
import datetime as dt
import json
import re
import sys
from pathlib import Path
from typing import Any

HEX64 = re.compile(r"^[0-9a-f]{64}$")
COMMIT = re.compile(r"^[0-9a-f]{40}$")
STATUSES = {"PASS", "FAIL", "BLOCKED_BY_ENVIRONMENT", "NOT_RUN", "INCOMPLETE"}
REQUIRED_CHECKS = {
    "candidate_identity", "architecture", "windows_build_policy", "disposable_test_user", "standard_user_token",
    "interactive_session", "false_interactive_claim_rejection", "real_logon", "logon_trigger",
    "setup_process_identity", "pipe_endpoint_discovery", "installed_binary_identity",
    "installed_process_identity", "pipe_protocol_v1", "pipe_ping", "pipe_wrong_user",
    "pipe_unsupported_version", "pipe_oversized_payload", "pipe_malformed_message",
    "pipe_expired_endpoint", "pipe_server_termination", "pipe_duplicate_connection",
    "pipe_stale_profile", "pipe_restart", "pipe_unauthorized_operation",
    "pipe_timeout_cancellation", "no_tcp_fallback", "setup_gui_launch", "gui_welcome",
    "gui_user_options", "gui_installation", "gui_completion", "installed_app_startup",
    "first_run_presentation", "no_unexpected_uac", "consistent_runtime_identity",
    "application_shutdown", "logon_expected_sid", "logon_no_elevated_token",
    "logon_startup_state", "logon_no_duplicate_processes", "logon_logout_shutdown",
    "install_root", "uninstall_preservation", "cleanup", "timeout_cancellation",
    "evidence_redaction", "no_wizard_execution_activation", "harness_revision", "component_results",
}


class EvidenceError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise EvidenceError(message)


def _hash(value: Any, label: str) -> None:
    require(isinstance(value, str) and HEX64.fullmatch(value) is not None,
            f"{label} must be a lowercase SHA-256")


def _status(value: Any, label: str) -> None:
    require(value in STATUSES, f"{label} has an unknown evidence status")


def _keys(value: Any, allowed: set[str], required: set[str], label: str) -> None:
    require(isinstance(value, dict), f"{label} must be an object")
    require(required <= value.keys(), f"{label} is incomplete")
    require(value.keys() <= allowed, f"{label} contains unknown fields")


def validate(record: dict[str, Any]) -> None:
    _keys(record, {
        "schema_version", "result", "execution_id", "executed_at_utc", "source_commit", "harness_source_commit",
        "candidate", "environment", "privilege", "session", "checks", "cleanup",
        "redaction", "identity_binding", "ui_evidence", "component_evidence_sha256",
    }, {
        "schema_version", "result", "execution_id", "executed_at_utc", "source_commit", "harness_source_commit",
        "candidate", "environment", "privilege", "session", "checks", "cleanup", "redaction", "ui_evidence",
        "component_evidence_sha256",
    }, "evidence")
    require(record.get("schema_version") == 1, "unsupported evidence schema")
    _status(record.get("result"), "result")
    require(COMMIT.fullmatch(str(record.get("source_commit", ""))) is not None,
            "source_commit must be a full commit SHA")
    require(COMMIT.fullmatch(str(record.get("harness_source_commit", ""))) is not None,
            "harness_source_commit must be a full commit SHA")
    try:
        timestamp = dt.datetime.fromisoformat(record["executed_at_utc"].replace("Z", "+00:00"))
        require(timestamp.tzinfo is not None, "execution timestamp must include a timezone")
    except (KeyError, TypeError, ValueError) as exc:
        raise EvidenceError("execution timestamp must be ISO-8601 with timezone") from exc
    require(bool(re.fullmatch(r"[A-Za-z0-9_-]{12,80}", str(record.get("execution_id", "")))),
            "execution_id must be an opaque bounded identifier")

    candidate = record.get("candidate")
    _keys(candidate, {
        "filename", "sha256", "size_bytes", "source_commit", "release_manifest_sha256",
        "signature_status", "expected_sha256", "expected_size_bytes", "expected_release_manifest_sha256",
    }, {
        "filename", "sha256", "size_bytes", "source_commit", "release_manifest_sha256",
        "signature_status", "expected_sha256", "expected_size_bytes", "expected_release_manifest_sha256",
    }, "candidate")
    _hash(candidate.get("sha256"), "candidate.sha256")
    _hash(candidate.get("release_manifest_sha256"), "candidate.release_manifest_sha256")
    require(candidate.get("filename") == "SynveilSetup.exe", "candidate filename mismatch")
    require(isinstance(candidate.get("size_bytes"), int) and candidate["size_bytes"] > 0,
            "candidate size must be positive")
    require(COMMIT.fullmatch(str(candidate.get("source_commit", ""))) is not None,
            "candidate source_commit must be a full commit SHA")
    initial_checks = record.get("checks")
    candidate_identity_passed = isinstance(initial_checks, dict) and initial_checks.get("candidate_identity") == "PASS"
    if candidate_identity_passed:
        require(candidate.get("source_commit") == record["source_commit"],
                "candidate source does not match evidence source")
    _hash(candidate.get("expected_sha256"), "candidate.expected_sha256")
    _hash(candidate.get("expected_release_manifest_sha256"),
          "candidate.expected_release_manifest_sha256")
    require(isinstance(candidate.get("expected_size_bytes"), int) and candidate["expected_size_bytes"] > 0,
            "expected candidate size must be positive")
    if candidate_identity_passed:
        require(candidate["sha256"] == candidate["expected_sha256"] and
                candidate["size_bytes"] == candidate["expected_size_bytes"] and
                candidate["release_manifest_sha256"] == candidate["expected_release_manifest_sha256"],
                "candidate identity PASS does not bind expected byte identity")
    require(candidate.get("signature_status") in {"SIGNED_VERIFIED_BY_APPROVED_POLICY", "UNSIGNED_FIXTURE"},
            "candidate signature classification is unsupported")

    env = record.get("environment")
    _keys(env, {"edition", "build", "architecture", "harness_revision_sha256", "qualification", "operator_approved"},
          {"edition", "build", "architecture", "harness_revision_sha256", "qualification", "operator_approved"}, "environment")
    require(env.get("edition") and isinstance(env.get("build"), int), "Windows edition/build missing")
    require(isinstance(env.get("architecture"), str) and env["architecture"], "Windows architecture missing")
    _hash(env.get("harness_revision_sha256"), "harness_revision_sha256")
    require(env.get("qualification") in {"github_hosted_service", "qualified_isolated_native", "unsupported"},
            "unknown environment qualification")
    require(isinstance(env.get("operator_approved"), bool), "operator approval evidence missing")

    privilege = record.get("privilege")
    _keys(privilege, {"category", "elevated"}, {"category", "elevated"}, "privilege")
    require(privilege.get("category") in {"standard_user", "administrator", "unknown"},
            "unknown privilege category")
    require(isinstance(privilege.get("elevated"), bool), "token elevation evidence is missing")
    require(not (privilege.get("category") == "standard_user" and privilege.get("elevated")),
            "standard-user evidence claims an elevated token")

    session = record.get("session")
    _keys(session, {
        "classification", "interactive_status", "new_logon_status", "trigger_status",
        "native_observations", "logon_observations",
    }, {
        "classification", "interactive_status", "new_logon_status", "trigger_status",
        "native_observations", "logon_observations",
    }, "session")
    _status(session.get("interactive_status"), "interactive session")
    _status(session.get("new_logon_status"), "new logon")
    _status(session.get("trigger_status"), "LogonTrigger")
    session_class = session.get("classification")
    require(session_class in {"genuine_interactive", "alternate_credentials_noninteractive", "service", "unsupported"},
            "unknown Windows session classification")
    sources = session.get("native_observations")
    native_observation_keys = {"input_desktop_open", "wts_session_active", "active_console_session_match",
                               "wts_session_has_user", "process_user_matches_wts_identity"}
    _keys(sources, native_observation_keys, native_observation_keys, "native_observations")
    for key in ("input_desktop_open", "wts_session_active", "active_console_session_match",
                "wts_session_has_user", "process_user_matches_wts_identity"):
        require(isinstance(sources.get(key), bool), f"session observation {key} missing")
    require(not (session_class == "genuine_interactive" and not all(
        sources[key] for key in ("input_desktop_open", "wts_session_active", "wts_session_has_user",
                                 "process_user_matches_wts_identity")
    )), "interactive-session claim lacks independent native observations")
    if session_class == "alternate_credentials_noninteractive":
        require(session.get("interactive_status") != "PASS", "alternate credentials cannot prove interaction")
    logon = session.get("logon_observations")
    _keys(logon, {"security_4624_matches_current_logon_luid", "logon_type_interactive", "task_logon_trigger_event"},
          {"security_4624_matches_current_logon_luid", "logon_type_interactive", "task_logon_trigger_event"}, "logon_observations")
    for key in ("security_4624_matches_current_logon_luid", "logon_type_interactive", "task_logon_trigger_event"):
        require(isinstance(logon.get(key), bool), f"logon observation {key} missing")
    if session.get("new_logon_status") == "PASS":
        require(logon["security_4624_matches_current_logon_luid"] and logon["logon_type_interactive"],
                "new-logon PASS is not bound to a matching interactive 4624 event")
    if session.get("trigger_status") == "PASS":
        require(logon["task_logon_trigger_event"], "LogonTrigger PASS lacks scheduler trigger evidence")

    checks = record.get("checks")
    require(isinstance(checks, dict), "acceptance checks missing")
    require(REQUIRED_CHECKS <= checks.keys(), "acceptance checks are incomplete")
    for name, status in checks.items():
        _status(status, f"checks.{name}")
    _keys(record["cleanup"], {"status", "independently_verified"}, {"status", "independently_verified"}, "cleanup")
    require(record.get("cleanup", {}).get("status") in STATUSES, "cleanup result missing")
    require(isinstance(record.get("cleanup", {}).get("independently_verified"), bool),
            "independent cleanup verification missing")
    _keys(record["redaction"], {"no_credentials_or_user_paths", "raw_pipe_payloads_recorded"},
          {"no_credentials_or_user_paths", "raw_pipe_payloads_recorded"}, "redaction")
    require(record.get("redaction", {}).get("no_credentials_or_user_paths") is True,
            "evidence redaction assertion missing")
    require(record.get("redaction", {}).get("raw_pipe_payloads_recorded") is False,
            "raw pipe messages must not be published")
    component_digests = record.get("component_evidence_sha256")
    component_names = {"environment", "gui", "logon", "logoff", "uninstall", "cleanup"}
    _keys(component_digests, component_names, component_names, "component_evidence_sha256")
    for name, value in component_digests.items():
        if value is not None:
            _hash(value, f"component_evidence_sha256.{name}")
    if "identity_binding" in record:
        _keys(record["identity_binding"], {"user_sid_sha256", "process_session_id"},
              {"user_sid_sha256", "process_session_id"}, "identity_binding")
        _hash(record["identity_binding"]["user_sid_sha256"], "identity_binding.user_sid_sha256")
        require(isinstance(record["identity_binding"]["process_session_id"], int),
                "identity_binding process session id is invalid")

    ui = record.get("ui_evidence")
    _keys(ui, {"mode", "state_assertions", "screenshots_status"},
          {"mode", "state_assertions", "screenshots_status"}, "ui_evidence")
    require(ui.get("mode") in {"native_ui_automation", "unavailable"}, "unknown UI evidence mode")
    require(isinstance(ui.get("state_assertions"), bool), "UI state assertion classification is missing")
    require(ui.get("screenshots_status") in {"CAPTURED", "NOT_CAPTURED", "BLOCKED_BY_ENVIRONMENT"},
            "unknown screenshot evidence status")
    if ui.get("screenshots_status") == "CAPTURED":
        require(ui.get("mode") == "native_ui_automation", "screenshots cannot replace native UI state assertions")

    if record["result"] == "PASS":
        require(env["qualification"] == "qualified_isolated_native", "hosted/unqualified evidence cannot be PASS")
        require(env["architecture"] == "x86_64", "PASS requires x86-64 Windows")
        require(env["operator_approved"], "PASS requires explicit operator approval for an isolated environment")
        require(session["interactive_status"] == "PASS" and session["new_logon_status"] == "PASS"
                and session["trigger_status"] == "PASS", "PASS requires interactive new-logon evidence")
        require(ui.get("mode") == "native_ui_automation" and ui.get("state_assertions"),
                "PASS requires actual native UI Automation state assertions")
        require(privilege["category"] == "standard_user" and not privilege["elevated"],
                "PASS requires a non-elevated standard-user token")
        require(all(checks[name] == "PASS" for name in REQUIRED_CHECKS), "PASS has missing or failed checks")
        require(record["cleanup"]["status"] == "PASS" and record["cleanup"]["independently_verified"],
                "PASS requires independently verified cleanup")
        require(all(value is not None for value in component_digests.values()),
                "PASS requires every component evidence digest")
        require("windows 11" in env["edition"].lower() and env["build"] >= 22000,
                "PASS requires a supported Windows 11 build")
    elif record["result"] == "BLOCKED_BY_ENVIRONMENT":
        require(env["qualification"] != "qualified_isolated_native" or
                any(checks[name] == "BLOCKED_BY_ENVIRONMENT" for name in REQUIRED_CHECKS),
                "blocked result has no blocked required check")
    elif record["result"] == "FAIL":
        require(any(value == "FAIL" for value in checks.values()) or record["cleanup"]["status"] == "FAIL",
                "FAIL has no failed scenario or cleanup result")
    elif record["result"] == "INCOMPLETE":
        require(any(value in {"NOT_RUN", "INCOMPLETE"} for value in checks.values()) or
                session["new_logon_status"] in {"NOT_RUN", "INCOMPLETE"} or
                session["trigger_status"] in {"NOT_RUN", "INCOMPLETE"},
                "INCOMPLETE has no incomplete evidence")

    # Public evidence is schema-bounded: forbid common accidental disclosure keys.
    def scan(value: Any, key: str = "") -> None:
        lowered = key.lower()
        allowed_redaction_flags = {"no_credentials_or_user_paths", "raw_pipe_payloads_recorded"}
        require(lowered in allowed_redaction_flags or not any(
            secret in lowered for secret in ("password", "credential", "token_value", "username", "user_path", "pipe_payload")
        ),
                "sensitive field name is forbidden in public evidence")
        if isinstance(value, dict):
            for child_key, child in value.items():
                scan(child, str(child_key))
        elif isinstance(value, list):
            for child in value:
                scan(child, key)
    scan(record)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("evidence", type=Path)
    args = parser.parse_args()
    try:
        record = json.loads(args.evidence.read_text(encoding="utf-8"))
        if not isinstance(record, dict):
            raise EvidenceError("evidence root must be an object")
        validate(record)
    except (OSError, json.JSONDecodeError, EvidenceError) as exc:
        print(f"windows native evidence: INVALID ({exc})", file=sys.stderr)
        return 1
    print(f"windows native evidence: VALID ({record['result']})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
