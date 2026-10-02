#!/usr/bin/env python3
"""Validate and inventory the versioned v0.2 installation acceptance contract."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
import platform as host_platform
import re
import shutil
import sys
import unittest
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
SCENARIOS = ROOT / "tests/install-acceptance/scenarios"
SCHEMA = ROOT / "tests/install-acceptance/schema/scenario-v1.schema.json"
VERSION = 1
RUNNER_VERSION = "0.1.0-contract"
AUTHORITATIVE_IDS = [*(f"INSTALL-JOURNEY-{i}" for i in range(1, 9)), "FIRST-RUN-1", "FIRST-RUN-2"]
PLATFORMS = {"windows", "debian", "ubuntu", "fedora", "rpm_family", "linux_generic"}
ARCHITECTURES = {"x86_64", "aarch64", "x86", "unknown"}
EXECUTION_CLASSES = {"STATIC", "FIXTURE", "CI_NATIVE", "NATIVE_CLEAN_MACHINE", "LIVE_EXTERNAL_DEPENDENCY", "PHYSICAL_OR_VM_INTERRUPTION"}
EVIDENCE = ["contract-valid", "static", "fixture", "ci-native-scoped", "native-clean-machine", "live-external-dependency", "interruption-power-cycle", "release-acceptance"]
CAPABILITIES = {"graphical_session", "native_package_manager", "administrator_elevation", "systemd_user", "systemd_system", "windows_task_scheduler", "secret_store", "network_access", "loopback_server", "postgresql", "reboot", "power_cycle", "release_download", "internet_access"}
ASSERTIONS = {"file_exists", "file_absent", "process_starts", "process_running", "process_not_running", "entrypoint_launches", "ipc_available", "startup_registration_enabled", "startup_registration_disabled", "package_installed", "package_removed", "version_equals", "state_preserved", "credential_preserved", "database_preserved", "library_preserved", "server_objects_preserved", "config_preserved", "health_ready", "sync_observed", "no_terminal_required", "no_unexpected_elevation", "administrator_elevation_expected", "secret_store_used", "no_public_control_listener", "installation_ready", "profile_ready", "authenticated", "library_ready", "server_ready", "unsupported_platform_rejected_safely", "unknown_schema_rejected_safely", "ambiguous_mutation_reconciled"}
RESOURCES = {"APPLICATION_CONFIG", "CREDENTIAL_STATE", "CLIENT_SYNC_STATE", "USER_LIBRARY_CONTENT", "SERVER_CONFIG", "SERVER_DATABASE", "SERVER_OBJECT_DATA"}
STEP_ACTIONS = {"obtain_artifact", "open_installer", "accept_terms_and_options", "install", "launch", "connect_to_server", "authenticate", "choose_library", "choose_local_folder", "observe_sync", "damage_package_owned_state", "repair", "uninstall", "interrupt_at_boundary", "rerun_and_reconcile", "host_setup", "create_first_admin", "upgrade_from_source", "assertion_checkpoint"}
MILESTONES = {"INSTALLATION_READY", "APPLICATION_LAUNCHED", "PROFILE_READY", "AUTHENTICATED", "LIBRARY_READY", "SYNC_STARTED", "SERVER_READY"}
OWNERS = {f"P{i:03}" for i in range(1, 49)}


class ContractError(ValueError):
    pass


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ContractError(f"{path.relative_to(ROOT)}: cannot parse JSON: {exc}") from exc


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ContractError(message)


def _validate_scenario(item: Any, source: Path) -> None:
    label = source.name
    require(isinstance(item, dict), f"{label}: scenario must be an object")
    version = item.get("schema_version")
    require(version == VERSION, f"{label}: unsupported or missing schema_version {version!r}; supported: {VERSION}")
    required = {"schema_version", "id", "title", "journey", "platform", "architecture", "execution_class", "preconditions", "required_capabilities", "capability_conditions", "artifact_requirements", "steps", "milestones", "assertions", "preservation_assertions", "forbidden_outcomes", "evidence_requirements", "timeout_policy", "cleanup_policy", "future_owner", "availability", "source_installation", "interruption_points", "recovery_classifications"}
    require(required <= item.keys(), f"{label}: missing fields {sorted(required - item.keys())}")
    require(set(item) <= required, f"{label}: unknown fields {sorted(set(item) - required)}")
    require(isinstance(item["id"], str) and item["id"], f"{label}: id must be non-empty")
    require(isinstance(item["title"], str) and isinstance(item["journey"], str), f"{label}: title and journey must be strings")
    p = item["platform"]
    require(isinstance(p, dict) and p.get("family") in PLATFORMS, f"{label}: unknown platform family")
    require(isinstance(p.get("qualified_versions"), list) and p["qualified_versions"], f"{label}: qualified_versions must explicitly include pending or exact versions")
    require(isinstance(p.get("applicable_families"), list) and p["family"] in p["applicable_families"] and all(x in PLATFORMS for x in p["applicable_families"]), f"{label}: invalid applicable_families")
    require(item["architecture"] in ARCHITECTURES, f"{label}: unknown architecture")
    if p["family"] == "windows":
        require(p.get("package_manager") is None and p.get("native_gui_package_handler") is None, f"{label}: Linux package-manager facts are invalid for Windows")
    else:
        require(isinstance(p.get("package_manager"), str) and isinstance(p.get("native_gui_package_handler"), str), f"{label}: Linux package-manager facts must be named or pending")
    require(item["architecture"] == "x86_64", f"{label}: v0.2 baseline scenarios currently qualify x86_64 only")
    require(item["execution_class"] in EXECUTION_CLASSES, f"{label}: unknown execution_class")
    require(item["availability"] in {"IMPLEMENTATION_PENDING", "IMPLEMENTED_UNQUALIFIED", "AVAILABLE"}, f"{label}: unknown availability")
    for key in ("preconditions", "required_capabilities", "capability_conditions", "steps", "milestones", "assertions", "preservation_assertions", "forbidden_outcomes", "interruption_points", "recovery_classifications"):
        require(isinstance(item[key], list), f"{label}: {key} must be an array")
    require(all(x in MILESTONES for x in item["milestones"]) and len(item["milestones"]) == len(set(item["milestones"])), f"{label}: unknown or duplicate milestone")
    if item["id"] == "INSTALL-JOURNEY-8":
        require(item["interruption_points"] == ["acquisition", "package_payload_mutation", "platform_integration", "verification"], f"{label}: interrupted-install boundary coverage is incomplete")
        require(set(item["recovery_classifications"]) == {"known_pre_mutation_failure", "known_partial_mutation", "OutcomeUnknown"}, f"{label}: interrupted recovery classifications are incomplete")
    else:
        require(not item["interruption_points"] and not item["recovery_classifications"], f"{label}: interruption fields are reserved for INSTALL-JOURNEY-8")
    require(all(isinstance(x, str) and x in CAPABILITIES for x in item["required_capabilities"]), f"{label}: unknown capability")
    for condition in item["capability_conditions"]:
        require(isinstance(condition, dict) and set(condition) == {"families", "require_all"}, f"{label}: malformed conditional capability requirement")
        require(isinstance(condition["families"], list) and condition["families"] and all(x in PLATFORMS for x in condition["families"]), f"{label}: invalid capability condition platform")
        require(isinstance(condition["require_all"], list) and condition["require_all"] and all(x in CAPABILITIES for x in condition["require_all"]), f"{label}: unknown conditional capability")
    require(isinstance(item["evidence_requirements"], dict), f"{label}: evidence_requirements must be an object")
    require(item["evidence_requirements"].get("minimum") in EVIDENCE, f"{label}: invalid minimum evidence")
    require(isinstance(item["evidence_requirements"].get("satisfies", []), list), f"{label}: evidence satisfies must be an array")
    require(all(x in EVIDENCE for x in item["evidence_requirements"].get("satisfies", [])), f"{label}: unknown evidence class")
    require(all(EVIDENCE.index(x) >= EVIDENCE.index(item["evidence_requirements"]["minimum"]) for x in item["evidence_requirements"].get("satisfies", [])), f"{label}: weaker evidence ordering is invalid")
    require(isinstance(item["artifact_requirements"], dict) and {"required", "artifact_type", "identity_fields"} <= item["artifact_requirements"].keys(), f"{label}: invalid artifact_requirements")
    for step in item["steps"]:
        require(isinstance(step, dict) and step.get("action") in STEP_ACTIONS and isinstance(step.get("id"), str) and isinstance(step.get("description"), str), f"{label}: invalid typed step")
        require(not any(k in step for k in ("command", "shell", "script", "executable")), f"{label}: arbitrary command execution is forbidden")
    step_ids = [x["id"] for x in item["steps"]]
    require(len(step_ids) == len(set(step_ids)), f"{label}: duplicate step id")
    for assertion in item["assertions"]:
        require(isinstance(assertion, dict) and assertion.get("type") in ASSERTIONS and isinstance(assertion.get("id"), str), f"{label}: unknown assertion type")
    for preservation in item["preservation_assertions"]:
        require(isinstance(preservation, dict) and preservation.get("resource") in RESOURCES and preservation.get("snapshot") in {"exists", "canonical_path_identity", "content_digest", "database_logical_fingerprint", "configuration_value_fingerprint", "credential_presence_identity_usable", "library_tree_digest", "record_count_logical_digest"}, f"{label}: invalid preservation resource/snapshot")
    for forbidden in item["forbidden_outcomes"]:
        require(isinstance(forbidden, dict) and forbidden.get("on_observed") == "FAIL", f"{label}: forbidden outcomes must fail acceptance")
    owner = item["future_owner"]
    require(isinstance(owner, dict) and all(isinstance(owner.get(k), list) and owner[k] for k in ("implementation", "acceptance")), f"{label}: future_owner needs implementation and acceptance owners")
    require(all(x in OWNERS for k in ("implementation", "acceptance") for x in owner[k]), f"{label}: invalid future owner prompt")
    source = item["source_installation"]
    if item["id"] == "UPGRADE-TEMPLATE-1":
        source_fields = {"source_product_version", "source_commit", "source_schema_version", "source_installation_type", "source_artifact_identity"}
        require(isinstance(source, dict) and set(source) == source_fields, f"{label}: upgrade source metadata is incomplete or has unknown fields")
        require(source["source_commit"] == "fa23232ff0154f627ebdd221ec5435134f177af0", f"{label}: v0.1.0 source commit fixture changed")
    else:
        require(source is None, f"{label}: source_installation is only valid for upgrade scenarios")
    require(isinstance(item["timeout_policy"], dict) and item["timeout_policy"].get("on_unknown_outcome") == "reconcile_before_retry", f"{label}: timeout policy must reconcile unknown outcomes")
    require(isinstance(item["cleanup_policy"], dict) and item["cleanup_policy"].get("ownership_scope") == "scenario_created_only", f"{label}: cleanup must be scenario-owned")


def validate_scenario(item: Any, source: Path) -> None:
    try:
        _validate_scenario(item, source)
    except (KeyError, TypeError, IndexError) as exc:
        raise ContractError(f"{source.name}: malformed scenario structure: {exc}") from exc


def discover() -> list[tuple[Path, dict[str, Any]]]:
    validate_schema_documents()
    paths = sorted(SCENARIOS.glob("*.json"), key=lambda p: p.name.encode("utf-8"))
    require(bool(paths), "no scenario definitions discovered")
    loaded = [(p, load_json(p)) for p in paths]
    seen: dict[str, str] = {}
    for path, item in loaded:
        validate_scenario(item, path)
        sid = item["id"]
        ensure_unique_id(seen, sid, path.name)
        seen[sid] = path.name
    missing = sorted(set(AUTHORITATIVE_IDS) - seen.keys())
    require(not missing, f"missing authoritative scenario IDs: {missing}")
    return sorted(loaded, key=lambda pair: pair[1]["id"].encode("utf-8"))


def ensure_unique_id(seen: dict[str, str], scenario_id: str, source: str) -> None:
    require(scenario_id not in seen, f"duplicate scenario id {scenario_id}: {seen.get(scenario_id)} and {source}")


def validate_schema_documents() -> None:
    scenario_schema = load_json(SCHEMA)
    result_schema = load_json(SCHEMA.parent / "result-v1.schema.json")
    require(scenario_schema.get("$schema") == "https://json-schema.org/draft/2020-12/schema", "scenario schema must declare JSON Schema 2020-12")
    require(scenario_schema.get("properties", {}).get("schema_version", {}).get("const") == VERSION, "scenario JSON Schema version does not match validator")
    require(scenario_schema.get("additionalProperties") is False, "scenario JSON Schema must reject unknown top-level fields")
    require(result_schema.get("$schema") == "https://json-schema.org/draft/2020-12/schema", "result schema must declare JSON Schema 2020-12")
    require(result_schema.get("properties", {}).get("schema_version", {}).get("const") == 1, "result schema version is unsupported")
    require(result_schema.get("properties", {}).get("result", {}).get("enum") == ["PASS", "FAIL", "SKIPPED", "BLOCKED", "ERROR"], "result schema must retain the finite result states")


def validate_result_record(record: Any) -> None:
    """Validate the finite, secret-free result-v1 record without third-party code."""
    require(isinstance(record, dict), "result record must be an object")
    required = {
        "schema_version", "scenario_id", "execution_class", "scenario_definition_digest",
        "runner_version", "start_time", "end_time", "platform_facts", "capability_results",
        "artifact_identity", "result", "completed_steps", "assertion_results",
        "evidence_classification", "reason", "diagnostics_redacted", "diagnostic_references",
        "cleanup_result",
    }
    require(required <= record.keys(), f"result record is missing fields: {sorted(required - record.keys())}")
    require(record["schema_version"] == 1, "result schema version is unsupported")
    require(isinstance(record["scenario_definition_digest"], str) and re.fullmatch(r"[0-9a-f]{64}", record["scenario_definition_digest"]), "invalid scenario definition digest")
    require(record["result"] in {"PASS", "FAIL", "SKIPPED", "BLOCKED", "ERROR"}, "invalid result state")
    require(record["evidence_classification"] in EVIDENCE, "invalid evidence classification")
    require(record["diagnostics_redacted"] is True, "diagnostics must be redacted")
    require(isinstance(record["platform_facts"], dict), "platform_facts must be an object")
    require(isinstance(record["capability_results"], list), "capability_results must be an array")
    for capability in record["capability_results"]:
        require(isinstance(capability, dict) and capability.get("status") in {"available", "unavailable", "unknown"}, "invalid capability result")
    artifact = record["artifact_identity"]
    require(isinstance(artifact, dict), "artifact_identity must be an object")
    artifact_required = {"status", "product_version", "source_commit", "artifact_type", "filename", "digest", "sha256", "size_bytes", "manifest_identity", "platform", "architecture", "release_channel", "trust_status"}
    require(artifact_required <= artifact.keys(), f"artifact identity is missing fields: {sorted(artifact_required - artifact.keys())}")
    if artifact["sha256"] is not None:
        require(isinstance(artifact["sha256"], str) and re.fullmatch(r"[0-9a-f]{64}", artifact["sha256"]), "invalid artifact sha256")
        require(artifact["digest"] == artifact["sha256"], "digest and sha256 must agree")
    if artifact["size_bytes"] is not None:
        require(isinstance(artifact["size_bytes"], int) and artifact["size_bytes"] >= 0, "invalid artifact size")
    require(isinstance(record["completed_steps"], list) and isinstance(record["assertion_results"], list), "step/assertion results must be arrays")
    require(isinstance(record["diagnostic_references"], list), "diagnostic_references must be an array")
    cleanup = record["cleanup_result"]
    require(isinstance(cleanup, dict) and cleanup.get("status") in {"not-run", "complete", "incomplete", "failed"}, "invalid cleanup result")


def definition_digest(item: dict[str, Any]) -> str:
    raw = json.dumps(item, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.sha256(raw).hexdigest()


def evidence_satisfies(actual: str, minimum: str) -> bool:
    """Return whether the ordered evidence class reaches the scenario minimum."""
    return actual in EVIDENCE and minimum in EVIDENCE and EVIDENCE.index(actual) >= EVIDENCE.index(minimum)


def classify_preflight(applicable: bool, implementation_available: bool, artifact_available: bool, capabilities_available: bool, adapter_available: bool) -> str:
    if not applicable:
        return "SKIPPED"
    if not implementation_available or not artifact_available or not capabilities_available or not adapter_available:
        return "BLOCKED"
    return "BLOCKED"  # P004 has no product-execution adapter, so preflight can never claim PASS.


def detect_host() -> dict[str, Any]:
    family = "windows" if os.name == "nt" else "linux_generic" if sys.platform.startswith("linux") else "unknown"
    distro_id = ""
    distro_version = None
    distro_like: list[str] = []
    if sys.platform.startswith("linux"):
        try:
            for line in Path("/etc/os-release").read_text(encoding="utf-8").splitlines():
                key, sep, value = line.partition("=")
                if not sep:
                    continue
                value = value.strip().strip('"').lower()
                if key == "ID": distro_id = value
                if key == "VERSION_ID": distro_version = value
                if key == "ID_LIKE": distro_like = value.split()
            if distro_id in {"debian", "ubuntu"} or {"debian", "ubuntu"} & set(distro_like): family = "ubuntu" if distro_id == "ubuntu" else "debian"
            elif distro_id == "fedora" or "fedora" in distro_like: family = "fedora"
            elif any(x in {"rhel", "centos", "rocky", "almalinux", "opensuse"} for x in [distro_id, *distro_like]): family = "rpm_family"
        except OSError:
            pass
    package_manager = next((x for x in ("apt-get", "dnf", "yum", "zypper", "pacman") if shutil.which(x)), None)
    desktop = bool(os.environ.get("DISPLAY") or os.environ.get("WAYLAND_DISPLAY") or os.name == "nt")
    machine = host_platform.machine().lower()
    architecture = "x86_64" if machine in {"x86_64", "amd64", "x64"} else "aarch64" if machine in {"aarch64", "arm64"} else machine
    facts = {"family": family, "version": distro_version, "architecture": architecture, "desktop_session": "available" if desktop else "unavailable", "package_manager": package_manager, "native_gui_package_handler": "unknown"}
    if family == "windows": facts["native_gui_package_handler"] = "windows installer shell"
    elif family in {"debian", "ubuntu"}: facts["native_gui_package_handler"] = "unknown" if desktop else None
    elif family in {"fedora", "rpm_family"}: facts["native_gui_package_handler"] = "unknown" if desktop else None
    return facts


def detect_capability(name: str, facts: dict[str, Any]) -> str:
    if name == "graphical_session": return "available" if facts["desktop_session"] == "available" else "unavailable"
    if name == "native_package_manager": return "available" if facts["package_manager"] else "unavailable"
    if name == "administrator_elevation": return "available" if (os.name == "nt" and os.environ.get("USERNAME", "").lower() == "administrator") or (hasattr(os, "geteuid") and os.geteuid() == 0) else "unknown"
    if name == "systemd_system": return "available" if Path("/run/systemd/system").exists() else "unavailable" if sys.platform.startswith("linux") else "unknown"
    if name == "systemd_user": return "available" if shutil.which("systemctl") and os.environ.get("XDG_RUNTIME_DIR") else "unavailable" if sys.platform.startswith("linux") else "unknown"
    if name == "windows_task_scheduler": return "unknown" if os.name == "nt" else "unavailable"
    if name in {"release_download", "internet_access", "network_access", "postgresql", "secret_store", "loopback_server", "reboot", "power_cycle"}: return "unknown"
    return "unknown"


def required_capabilities_for_host(item: dict[str, Any], facts: dict[str, Any]) -> list[str]:
    required = list(item["required_capabilities"])
    host_family = facts["family"]
    match_families = {host_family, "debian"} if host_family == "ubuntu" else {host_family}
    for condition in item["capability_conditions"]:
        if match_families.intersection(condition["families"]):
            required.extend(condition["require_all"])
    return sorted(set(required))


def emit_inventory(items: list[tuple[Path, dict[str, Any]]]) -> None:
    records = []
    for _, s in items:
        records.append({"id": s["id"], "title": s["title"], "platform": s["platform"]["applicable_families"], "minimum_evidence": s["evidence_requirements"]["minimum"], "implementation_owner": s["future_owner"]["implementation"], "acceptance_owner": s["future_owner"]["acceptance"], "current_availability": s["availability"], "definition_digest": definition_digest(s)})
    print(json.dumps({"schema_version": 1, "scenarios": records}, ensure_ascii=False, sort_keys=True, indent=2))


def blocked_result(item: dict[str, Any]) -> dict[str, Any]:
    # P004 supplies contract validation only; no installer execution adapter is enabled.
    facts = detect_host()
    applicable_families = set(item["platform"]["applicable_families"])
    family_matches = ("linux_generic" in applicable_families and facts["family"] not in {"windows", "unknown"}) or facts["family"] in applicable_families or (facts["family"] == "ubuntu" and "debian" in applicable_families)
    matches = family_matches and facts["architecture"] == item["architecture"]
    required_capabilities = required_capabilities_for_host(item, facts)
    capabilities = [{"name": capability, "status": detect_capability(capability, facts)} for capability in required_capabilities]
    applicable = matches or facts["family"] == "unknown" or facts["architecture"] == "unknown"
    result = classify_preflight(applicable, False, False, not any(x["status"] != "available" for x in capabilities), False)
    reason = "Scenario platform does not apply to the detected host." if result == "SKIPPED" else "P004 defines the contract; no native execution adapter or qualified artifact is available. Required capability states are recorded, but cannot establish product acceptance."
    if result == "BLOCKED" and any(x["status"] in {"unavailable", "unknown"} for x in capabilities):
        reason += " One or more required capabilities are unavailable or unknown."
    timestamp = datetime.now(timezone.utc).isoformat()
    record = {"schema_version": 1, "scenario_id": item["id"], "execution_class": item["execution_class"], "scenario_definition_digest": definition_digest(item), "runner_version": RUNNER_VERSION, "start_time": timestamp, "end_time": timestamp, "platform_facts": facts, "capability_results": capabilities, "artifact_identity": {"status": "unavailable", "product_version": None, "source_commit": None, "artifact_type": item["artifact_requirements"]["artifact_type"], "filename": None, "digest": None, "sha256": None, "size_bytes": None, "manifest_identity": None, "platform": facts["family"], "architecture": facts["architecture"], "release_channel": None, "trust_status": "unknown"}, "result": result, "completed_steps": [], "assertion_results": [], "evidence_classification": "contract-valid", "reason": reason, "diagnostics_redacted": True, "diagnostic_references": [], "cleanup_result": {"status": "not-run", "details": None}}
    validate_result_record(record)
    return record


class ContractTests(unittest.TestCase):
    def test_valid_scenario_parses(self) -> None:
        entries = discover()
        validate_scenario(entries[0][1], entries[0][0])

    def test_duplicate_id_rejected(self) -> None:
        seen = {"DUP": "first-run-1.json"}
        with self.assertRaises(ContractError):
            ensure_unique_id(seen, "DUP", "second.json")

    def test_future_schema_rejected(self) -> None:
        item = dict(discover()[0][1]); item["schema_version"] = 999
        with self.assertRaises(ContractError): validate_scenario(item, SCENARIOS / "x.json")

    def test_missing_required_field_rejected(self) -> None:
        item = dict(discover()[0][1]); item.pop("steps")
        with self.assertRaises(ContractError): validate_scenario(item, SCENARIOS / "x.json")

    def test_malformed_field_type_rejected_clearly(self) -> None:
        item = dict(discover()[0][1]); item["architecture"] = []
        with self.assertRaises(ContractError): validate_scenario(item, SCENARIOS / "x.json")

    def test_unknown_enum_rejected(self) -> None:
        item = dict(discover()[0][1]); item["execution_class"] = "MAGIC"
        with self.assertRaises(ContractError): validate_scenario(item, SCENARIOS / "x.json")

    def test_invalid_platform_architecture_rejected(self) -> None:
        item = dict(discover()[0][1]); item["architecture"] = "mips"
        with self.assertRaises(ContractError): validate_scenario(item, SCENARIOS / "x.json")

    def test_invalid_platform_package_manager_pair_rejected(self) -> None:
        item = dict(discover()[0][1]); item["platform"] = dict(item["platform"]); item["platform"].update({"family": "windows", "package_manager": "apt"})
        with self.assertRaises(ContractError): validate_scenario(item, SCENARIOS / "x.json")

    def test_unknown_capability_rejected(self) -> None:
        item = dict(discover()[0][1]); item["required_capabilities"] = ["magic"]
        with self.assertRaises(ContractError): validate_scenario(item, SCENARIOS / "x.json")

    def test_platform_conditional_capability_is_scoped(self) -> None:
        scenario = next(s for _, s in discover() if s["id"] == "INSTALL-JOURNEY-6")
        self.assertNotIn("native_package_manager", required_capabilities_for_host(scenario, {"family": "windows"}))
        self.assertIn("native_package_manager", required_capabilities_for_host(scenario, {"family": "ubuntu"}))

    def test_weaker_evidence_cannot_claim_stronger_satisfaction(self) -> None:
        item = dict(discover()[0][1]); item["evidence_requirements"] = {"minimum": "native-clean-machine", "satisfies": ["fixture"]}
        with self.assertRaises(ContractError): validate_scenario(item, SCENARIOS / "x.json")

    def test_fixture_cannot_satisfy_native_clean_machine(self) -> None:
        self.assertFalse(evidence_satisfies("fixture", "native-clean-machine"))
        self.assertTrue(evidence_satisfies("native-clean-machine", "native-clean-machine"))

    def test_blocked_differs_from_skipped(self) -> None:
        blocked = blocked_result(discover()[0][1])
        self.assertIn(blocked["result"], {"BLOCKED", "SKIPPED"})
        self.assertTrue(all(x["status"] in {"available", "unavailable", "unknown"} for x in blocked["capability_results"]))
        self.assertNotEqual(blocked["result"], "PASS")
        self.assertEqual(classify_preflight(False, False, False, False, False), "SKIPPED")
        self.assertEqual(classify_preflight(True, False, False, True, True), "BLOCKED")

    def test_result_record_has_versioned_finite_shape(self) -> None:
        result = blocked_result(discover()[0][1])
        schema = load_json(ROOT / "tests/install-acceptance/schema/result-v1.schema.json")
        self.assertEqual(schema["properties"]["result"]["enum"], ["PASS", "FAIL", "SKIPPED", "BLOCKED", "ERROR"])
        self.assertTrue(set(schema["required"]) <= result.keys())
        self.assertTrue(result["diagnostics_redacted"])

    def test_forbidden_outcome_is_explicit(self) -> None:
        self.assertTrue(any("FAIL" in x["on_observed"] for _, s in discover() for x in s["forbidden_outcomes"]))

    def test_preservation_resource_owner_class_validated(self) -> None:
        item = dict(discover()[0][1]); item["preservation_assertions"] = [{"id": "x", "resource": "ARBITRARY", "snapshot": "exists"}]
        with self.assertRaises(ContractError): validate_scenario(item, SCENARIOS / "x.json")

    def test_discovery_is_deterministic(self) -> None:
        self.assertEqual([x[1]["id"] for x in discover()], [x[1]["id"] for x in discover()])
        self.assertEqual([x[1]["id"] for x in discover()], sorted(x[1]["id"] for x in discover()))

    def test_authoritative_ids_exactly_once(self) -> None:
        ids = [x[1]["id"] for x in discover()]
        self.assertEqual({x for x in ids if x in AUTHORITATIVE_IDS}, set(AUTHORITATIVE_IDS))
        self.assertEqual(len(ids), len(set(ids)))

    def test_future_owner_prompts_validated(self) -> None:
        self.assertTrue(all(set(s["future_owner"][k]) <= OWNERS for _, s in discover() for k in ("implementation", "acceptance")))

    def test_broken_owner_reference_rejected(self) -> None:
        item = dict(discover()[0][1]); item["future_owner"] = {"implementation": ["P999"], "acceptance": ["P028"]}
        with self.assertRaises(ContractError): validate_scenario(item, SCENARIOS / "x.json")

    def test_no_scenario_embeds_secret_values(self) -> None:
        for _, item in discover():
            serialized = json.dumps(item).lower()
            self.assertNotRegex(serialized, r"(password|token|secret_value)\s*[=:]")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("validate", help="validate definitions without mutating the host")
    sub.add_parser("inventory", help="emit deterministic machine-readable inventory")
    run = sub.add_parser("run", help="classify platform/capability preflight; product execution remains blocked")
    run.add_argument("scenario_id")
    args = parser.parse_args()
    try:
        items = discover()
        if args.command == "validate":
            print(f"acceptance contract valid: {len(items)} scenarios; schema v{VERSION}; native execution not performed")
        elif args.command == "inventory":
            emit_inventory(items)
        else:
            found = [item for _, item in items if item["id"] == args.scenario_id]
            if not found:
                raise ContractError(f"unknown scenario id: {args.scenario_id}")
            print(json.dumps(blocked_result(found[0]), sort_keys=True, indent=2))
        return 0
    except ContractError as exc:
        print(f"acceptance contract error: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "self-test":
        suite = unittest.defaultTestLoader.loadTestsFromTestCase(ContractTests)
        result = unittest.TextTestRunner(verbosity=2).run(suite)
        raise SystemExit(not result.wasSuccessful())
    raise SystemExit(main())
