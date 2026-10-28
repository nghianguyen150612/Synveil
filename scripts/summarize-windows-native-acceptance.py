#!/usr/bin/env python3
"""Bind P013 component records into one fail-closed NativeAcceptanceSummary."""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import importlib.util
import json
import re
import subprocess
import sys
import uuid
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
STATUSES = {"PASS", "FAIL", "BLOCKED_BY_ENVIRONMENT", "NOT_RUN", "INCOMPLETE"}
HEX64 = re.compile(r"^[0-9a-f]{64}$")
COMMIT = re.compile(r"^[0-9a-f]{40}$")
COMPONENT_FILES = {
    "environment": "windows-native-environment-v1.json",
    "gui": "windows-gui-journey-v1.json",
    "logon": "windows-new-logon-v1.json",
    "logoff": "windows-logoff-shutdown-v1.json",
    "uninstall": "windows-uninstall-preservation-v1.json",
    "cleanup": "windows-p013-cleanup-v1.json",
}
HARNESS_FILES = (
    "scripts/validate-windows-native-evidence.py",
    "scripts/qualify-windows-native-environment.ps1",
    "scripts/test-windows-gui-journey.ps1",
    "crates/client/examples/p013_windows_control_probe.rs",
    "scripts/test-windows-installed-uninstall.ps1",
    "scripts/collect-windows-new-logon-evidence.ps1",
    "scripts/collect-windows-logoff-evidence.ps1",
    "scripts/provision-windows-p013-test-user.ps1",
    "scripts/cleanup-windows-p013-test-user.ps1",
    "scripts/summarize-windows-native-acceptance.py",
)
COMPONENT_HARNESS_FILES = {
    "environment": ("scripts/qualify-windows-native-environment.ps1",),
    "gui": (
        "scripts/test-windows-gui-journey.ps1",
        "crates/client/examples/p013_windows_control_probe.rs",
    ),
    "logon": ("scripts/collect-windows-new-logon-evidence.ps1",),
    "logoff": ("scripts/collect-windows-logoff-evidence.ps1",),
    "uninstall": (
        "scripts/test-windows-installed-uninstall.ps1",
        "crates/client/examples/p013_windows_control_probe.rs",
    ),
    "cleanup": (
        "scripts/cleanup-windows-p013-test-user.ps1",
        "scripts/provision-windows-p013-test-user.ps1",
    ),
}


def get(value: Any, *path: str, default: Any = None) -> Any:
    for item in path:
        if not isinstance(value, dict) or item not in value:
            return default
        value = value[item]
    return value


def status(value: Any) -> str:
    return value if value in STATUSES else "NOT_RUN"


def combine(*values: Any) -> str:
    statuses = [status(value) for value in values]
    if "FAIL" in statuses:
        return "FAIL"
    if "BLOCKED_BY_ENVIRONMENT" in statuses:
        return "BLOCKED_BY_ENVIRONMENT"
    if "INCOMPLETE" in statuses:
        return "INCOMPLETE"
    if "NOT_RUN" in statuses:
        return "NOT_RUN"
    return "PASS"


def read_json(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8-sig"))
    if not isinstance(value, dict):
        raise ValueError(f"{path.name} must contain a JSON object")
    return value


def digest_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def harness_digest() -> str:
    rows = []
    for relative in HARNESS_FILES:
        path = ROOT / relative
        rows.append(f"{relative}:{digest_file(path)}")
    return hashlib.sha256("\n".join(sorted(rows)).encode()).hexdigest()


def component_harness_digest(name: str) -> str:
    paths = COMPONENT_HARNESS_FILES[name]
    rows = [f"{relative}:{digest_file(ROOT / relative)}" for relative in paths]
    if name == "gui":
        rows.sort()
    return hashlib.sha256("\n".join(rows).encode()).hexdigest() if len(rows) > 1 else rows[0].split(":", 1)[1]


def current_source_matches(expected: str) -> bool:
    try:
        head = subprocess.run(
            ["git", "-C", str(ROOT), "rev-parse", "HEAD"],
            check=True, capture_output=True, text=True,
        ).stdout.strip()
        dirty = subprocess.run(
            ["git", "-C", str(ROOT), "status", "--porcelain", "--untracked-files=all"],
            check=True, capture_output=True, text=True,
        ).stdout
    except (OSError, subprocess.CalledProcessError):
        return False
    return head == expected and not dirty.strip()


def verify_candidate(identity: dict[str, Any], setup: Path, release_path: Path, source: str) -> tuple[dict[str, Any], bool]:
    release = read_json(release_path)
    setup_sha = digest_file(setup)
    setup_size = setup.stat().st_size
    manifest_sha = digest_file(release_path)
    artifact = [
        entry for entry in release.get("artifacts", [])
        if entry.get("id") == "windows-x86_64-installer" and entry.get("filename") == "SynveilSetup.exe"
    ]
    identity_ok = (
        identity.get("filename") == "SynveilSetup.exe"
        and identity.get("source_commit") == source
        and identity.get("sha256") == setup_sha
        and identity.get("size_bytes") == setup_size
        and identity.get("release_manifest_sha256") == manifest_sha
        and identity.get("signature_status") == "UNSIGNED"
        and release.get("schema_version") == 1
        and release.get("product") == "Synveil"
        and release.get("source_commit") == source
        and len(artifact) == 1
        and artifact[0].get("sha256") == setup_sha
        and artifact[0].get("size_bytes") == setup_size
    )
    candidate = {
        "filename": "SynveilSetup.exe",
        "sha256": setup_sha,
        "size_bytes": setup_size,
        "source_commit": str(release.get("source_commit", source)),
        "release_manifest_sha256": manifest_sha,
        "expected_sha256": str(identity.get("sha256", setup_sha)),
        "expected_size_bytes": int(identity.get("size_bytes", setup_size)),
        "expected_release_manifest_sha256": str(identity.get("release_manifest_sha256", manifest_sha)),
        "signature_status": "UNSIGNED_FIXTURE",
    }
    return candidate, identity_ok


def load_components(directory: Path) -> tuple[dict[str, dict[str, Any]], dict[str, str | None]]:
    components: dict[str, dict[str, Any]] = {}
    hashes: dict[str, str | None] = {}
    for name, filename in COMPONENT_FILES.items():
        path = directory / filename
        if not path.is_file():
            hashes[name] = None
            continue
        components[name] = read_json(path)
        hashes[name] = digest_file(path)
    return components, hashes


def component_candidate_matches(component: dict[str, Any], source: str, candidate: dict[str, Any]) -> bool:
    item = component.get("candidate")
    if not isinstance(item, dict):
        return False
    return (
        component.get("source_commit") == source
        and item.get("sha256") == candidate["sha256"]
        and item.get("size_bytes") == candidate["size_bytes"]
        and item.get("release_manifest_sha256") == candidate["release_manifest_sha256"]
    )


def owner_checks(gui: dict[str, Any]) -> dict[str, Any]:
    return get(gui, "control_ipc", "owner", "checks", default={}) or {}


def make_summary(components: dict[str, dict[str, Any]], component_hashes: dict[str, str | None],
                 candidate: dict[str, Any], identity_ok: bool, source: str, harness_source: str,
                 required_checks: set[str]) -> dict[str, Any]:
    env = components.get("environment", {})
    gui = components.get("gui", {})
    logon = components.get("logon", {})
    logoff = components.get("logoff", {})
    uninstall = components.get("uninstall", {})
    cleanup = components.get("cleanup", {})
    env_checks = get(env, "checks", default={}) or {}
    gui_states = get(gui, "product_state", default={}) or {}
    ipc = get(gui, "control_ipc", default={}) or {}
    owner = owner_checks(gui)
    logon_checks = get(logon, "checks", default={}) or {}
    logoff_checks = get(logoff, "checks", default={}) or {}
    uninstall_checks = get(uninstall, "checks", default={}) or {}
    cleanup_checks = get(cleanup, "checks", default={}) or {}

    trigger = combine(
        logon_checks.get("exact_logon_trigger_definition"),
        logon_checks.get("scheduler_logon_trigger_event"),
        logon_checks.get("task_started_after_logon"),
        logon_checks.get("client_started_from_task"),
        logon_checks.get("scheduler_manual_run_absent"),
    )
    new_logon = combine(logon_checks.get("matching_security_4624"), logon_checks.get("interactive_logon_type"))
    environment_session = get(env, "session", default={}) or {}
    native_observations = get(environment_session, "native_observations", default={}) or {}
    logon_observations = {
        "security_4624_matches_current_logon_luid": logon_checks.get("matching_security_4624") == "PASS",
        "logon_type_interactive": logon_checks.get("interactive_logon_type") == "PASS",
        "task_logon_trigger_event": (
            logon_checks.get("exact_logon_trigger_definition") == "PASS"
            and logon_checks.get("scheduler_logon_trigger_event") == "PASS"
            and logon_checks.get("scheduler_manual_run_absent") == "PASS"
        ),
    }

    checks = {name: "NOT_RUN" for name in required_checks}
    present_harnesses = [
        (name, component.get("harness_revision_sha256"))
        for name, component in components.items()
    ]
    harness_records_match = all(
        get(components.get(name, {}), "harness_source_commit") == harness_source
        for name in ("gui", "uninstall")
    )
    harness_revision_status = (
        "NOT_RUN" if len(components) != len(COMPONENT_FILES)
        else "PASS" if current_source_matches(harness_source) and harness_records_match and
        all(digest == component_harness_digest(name) for name, digest in present_harnesses)
        else "FAIL"
    )
    identity_hashes = [
        get(env, "identity_binding", "user_sid_sha256"),
        gui.get("user_sid_sha256"),
        get(logon, "session", "user_sid_sha256"),
        get(logoff, "session", "user_sid_sha256"),
        get(uninstall, "session", "user_sid_sha256"),
    ]
    present_identity_hashes = [value for value in identity_hashes if value is not None]
    if any(not isinstance(value, str) or not HEX64.fullmatch(value) for value in present_identity_hashes):
        identity_binding_status = "FAIL"
    elif len(set(present_identity_hashes)) > 1:
        identity_binding_status = "FAIL"
    elif len(present_identity_hashes) != len(identity_hashes):
        identity_binding_status = "NOT_RUN"
    else:
        identity_binding_status = "PASS"
    runtime_identity = combine(
        identity_binding_status,
        gui_states.get("installed_client_process"),
        logon_checks.get("client_expected_sid"),
        uninstall_checks.get("interactive_standard_user"),
    )
    checks.update({
        "candidate_identity": "PASS" if identity_ok else "FAIL",
        "architecture": status(env_checks.get("architecture")),
        "windows_build_policy": status(env_checks.get("windows_build_policy")),
        "disposable_test_user": status(env_checks.get("disposable_test_user")),
        "standard_user_token": status(env_checks.get("standard_user_token")),
        "interactive_session": status(environment_session.get("interactive_status")),
        "false_interactive_claim_rejection": status(env_checks.get("false_interactive_claim_rejection")),
        "real_logon": new_logon,
        "logon_trigger": trigger,
        "setup_process_identity": combine(gui_states.get("setup_as_standard_user"), gui_states.get("setup_non_elevated")),
        "pipe_endpoint_discovery": status(owner.get("pipe_endpoint_discovery")),
        "installed_binary_identity": status(gui_states.get("installed_runtime")),
        "installed_process_identity": combine(gui_states.get("installed_launch"), gui_states.get("installed_client_process"),
                                               gui_states.get("installed_desktop_non_elevated"), gui_states.get("installed_client_non_elevated"),
                                               gui_states.get("installed_desktop_ipc_ping")),
        "pipe_protocol_v1": status(owner.get("pipe_protocol_v1")),
        "pipe_ping": status(owner.get("pipe_ping")),
        "pipe_wrong_user": status(gui_states.get("pipe_wrong_user")),
        "pipe_unsupported_version": status(owner.get("pipe_unsupported_version")),
        "pipe_oversized_payload": status(owner.get("pipe_oversized_payload")),
        "pipe_malformed_message": status(owner.get("pipe_malformed_message")),
        "pipe_expired_endpoint": status(gui_states.get("pipe_expired_endpoint")),
        "pipe_server_termination": status(gui_states.get("pipe_server_termination")),
        "pipe_duplicate_connection": status(owner.get("pipe_duplicate_connection")),
        "pipe_stale_profile": status(gui_states.get("pipe_stale_profile")),
        "pipe_restart": status(gui_states.get("pipe_restart")),
        "pipe_unauthorized_operation": status(gui_states.get("pipe_unauthorized_operation")),
        "pipe_timeout_cancellation": status(owner.get("pipe_timeout_cancellation")),
        "no_tcp_fallback": status(gui_states.get("no_tcp_fallback")),
        "setup_gui_launch": status(gui_states.get("setup_as_standard_user")),
        "gui_welcome": status(gui_states.get("welcome")),
        "gui_user_options": status(gui_states.get("user_options")),
        "gui_installation": combine(gui_states.get("install_location"), gui_states.get("install_confirmation"), gui_states.get("install_progress")),
        "gui_completion": status(gui_states.get("completion")),
        "installed_app_startup": status(gui_states.get("installed_launch")),
        "first_run_presentation": status(gui_states.get("first_run")),
        "no_unexpected_uac": combine(gui_states.get("unexpected_uac"), gui_states.get("setup_non_elevated"),
                                      gui_states.get("installed_desktop_non_elevated"), gui_states.get("installed_client_non_elevated")),
        "consistent_runtime_identity": runtime_identity,
        "application_shutdown": combine(gui_states.get("shutdown"), gui_states.get("pipe_server_termination")),
        "logon_expected_sid": status(logon_checks.get("client_expected_sid")),
        "logon_no_elevated_token": status(logon_checks.get("client_non_elevated")),
        "logon_startup_state": combine(logon_checks.get("exact_logon_trigger_definition"), logon_checks.get("scheduler_logon_trigger_event"), logon_checks.get("scheduler_manual_run_absent")),
        "logon_no_duplicate_processes": status(logon_checks.get("no_duplicate_client")),
        "logon_logout_shutdown": combine(logoff.get("logoff_shutdown"), logoff_checks.get("client_absent_after_logoff"), logoff_checks.get("independent_process_check")),
        "install_root": status(gui_states.get("install_location")),
        "uninstall_preservation": status(uninstall_checks.get("uninstall_preservation")),
        "cleanup": combine(cleanup.get("cleanup", {}).get("status"), "PASS" if get(cleanup, "cleanup", "independently_verified") is True else "FAIL" if get(cleanup, "cleanup", "status") == "FAIL" else "NOT_RUN"),
        "timeout_cancellation": status(owner.get("pipe_timeout_cancellation")),
        "evidence_redaction": "PASS" if (
            get(env, "redaction", "no_credentials_or_user_paths") is True
            and get(env, "redaction", "raw_pipe_payloads_recorded") is False
            and gui.get("redacted") is True
            and get(logon, "redaction", "raw_event_data_recorded") is False
            and get(logoff, "redaction", "raw_event_data_recorded") is False
            and get(uninstall, "redaction", "credentials_recorded") is False
            and get(uninstall, "redaction", "raw_pipe_messages_recorded") is False
        ) else "NOT_RUN",
        "no_wizard_execution_activation": status(env_checks.get("no_wizard_execution_activation")),
        "harness_revision": harness_revision_status,
        "component_results": combine(
            env.get("result"), gui.get("result"), logon.get("result"),
            logoff.get("result"), uninstall.get("result"), cleanup.get("result"),
        ),
    })

    result = "PASS"
    if any(value == "FAIL" for value in checks.values()):
        result = "FAIL"
    elif any(value == "BLOCKED_BY_ENVIRONMENT" for value in checks.values()):
        result = "BLOCKED_BY_ENVIRONMENT"
    elif any(value in {"NOT_RUN", "INCOMPLETE"} for value in checks.values()):
        result = "INCOMPLETE"

    edition = get(env, "windows_edition") or get(gui, "windows_edition") or "unavailable"
    build = get(env, "windows_build") or get(gui, "windows_build") or 0
    architecture = get(env, "architecture") or get(gui, "architecture") or "unknown"
    qualification = get(env, "environment", "qualification", default="unsupported")
    elevated = bool(get(env, "privilege", "elevated", default=False))
    category = get(env, "privilege", "category", default="unknown")
    ui = get(gui, "ui_evidence", default={}) or {
        "mode": "unavailable", "state_assertions": False, "screenshots_status": "BLOCKED_BY_ENVIRONMENT"
    }
    session_id = get(env, "identity_binding", "process_session_id")
    user_hash = get(env, "identity_binding", "user_sid_sha256")
    summary: dict[str, Any] = {
        "schema_version": 1,
        "result": result,
        "execution_id": "p013-summary-" + uuid.uuid4().hex,
        "executed_at_utc": dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z"),
        "source_commit": source,
        "harness_source_commit": harness_source,
        "candidate": candidate,
        "environment": {
            "edition": str(edition), "build": int(build) if isinstance(build, (int, str)) and str(build).isdigit() else 0,
            "architecture": str(architecture), "harness_revision_sha256": harness_digest(),
            "qualification": qualification if qualification in {"github_hosted_service", "qualified_isolated_native", "unsupported"} else "unsupported",
            "operator_approved": bool(get(env, "environment", "operator_approved", default=False)),
        },
        "privilege": {"category": category if category in {"standard_user", "administrator", "unknown"} else "unknown", "elevated": elevated},
        "session": {
            "classification": get(environment_session, "classification", default="unsupported"),
            "interactive_status": status(environment_session.get("interactive_status")),
            "new_logon_status": new_logon,
            "trigger_status": trigger,
            "native_observations": {
                name: bool(native_observations.get(name, False)) for name in (
                    "input_desktop_open", "wts_session_active", "active_console_session_match",
                    "wts_session_has_user", "process_user_matches_wts_identity",
                )
            },
            "logon_observations": logon_observations,
        },
        "checks": checks,
        "cleanup": {
            "status": status(get(cleanup, "cleanup", "status")),
            "independently_verified": get(cleanup, "cleanup", "independently_verified") is True,
        },
        "redaction": {"no_credentials_or_user_paths": True, "raw_pipe_payloads_recorded": False},
        "ui_evidence": {
            "mode": ui.get("mode", "unavailable"),
            "state_assertions": bool(ui.get("state_assertions", False)),
            "screenshots_status": ui.get("screenshots_status", "BLOCKED_BY_ENVIRONMENT"),
        },
        "component_evidence_sha256": component_hashes,
    }
    if isinstance(user_hash, str) and HEX64.fullmatch(user_hash) and isinstance(session_id, int):
        summary["identity_binding"] = {"user_sid_sha256": user_hash, "process_session_id": session_id}
    return summary


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--candidate-identity", type=Path, required=True)
    parser.add_argument("--setup", type=Path, required=True)
    parser.add_argument("--release-manifest", type=Path, required=True)
    parser.add_argument("--source-commit", required=True)
    parser.add_argument("--harness-commit", required=True)
    parser.add_argument("--evidence-directory", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if not COMMIT.fullmatch(args.source_commit):
        print("P013_SOURCE_COMMIT_INVALID", file=sys.stderr)
        return 2
    if not COMMIT.fullmatch(args.harness_commit):
        print("P013_HARNESS_COMMIT_INVALID", file=sys.stderr)
        return 2
    try:
        identity = read_json(args.candidate_identity)
        candidate, identity_ok = verify_candidate(identity, args.setup, args.release_manifest, args.source_commit)
        components, component_hashes = load_components(args.evidence_directory)
        for component in components.values():
            if not component_candidate_matches(component, args.source_commit, candidate):
                identity_ok = False
        contract_path = ROOT / "scripts/validate-windows-native-evidence.py"
        spec = importlib.util.spec_from_file_location("validate_windows_native_evidence", contract_path)
        if spec is None or spec.loader is None:
            raise RuntimeError("evidence validator cannot be loaded")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        summary = make_summary(components, component_hashes, candidate, identity_ok,
                               args.source_commit, args.harness_commit, module.REQUIRED_CHECKS)
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        module.validate(summary)
    except (OSError, ValueError, TypeError, KeyError, AttributeError, json.JSONDecodeError,
            subprocess.SubprocessError, RuntimeError) as exc:
        print(f"P013_SUMMARY_INCOMPLETE: {exc}", file=sys.stderr)
        return 1
    print(f"P013 native acceptance summary: {summary['result']}")
    print(f"Evidence: {args.output}")
    return 0 if summary["result"] == "PASS" else 2 if summary["result"] == "BLOCKED_BY_ENVIRONMENT" else 1


if __name__ == "__main__":
    raise SystemExit(main())
