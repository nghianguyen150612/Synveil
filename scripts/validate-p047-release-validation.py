#!/usr/bin/env python3
"""Validate the P047 release-candidate evidence inventory without inferring PASS."""
from __future__ import annotations

import json
import re
import sys
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
REPORT = ROOT / "acceptance/p047/release-candidate-validation-v1.json"
RESULTS = {"PASS", "FAIL", "BLOCKED", "ERROR", "SKIPPED"}
ARTIFACT_STATES = {"AVAILABLE", "MISSING", "INVALID", "UNVERIFIED", "BLOCKED"}
EVIDENCE_CLASSES = {
    "contract-valid", "static", "fixture", "ci-native-scoped",
    "native-clean-machine", "live-external-dependency",
    "interruption-power-cycle", "release-acceptance",
}
SHA1 = re.compile(r"^[0-9a-f]{40}$")
SHA256 = re.compile(r"^[0-9a-f]{64}$")
COVERAGE = {
    "SOURCE_QUALITY", "DESKTOP_RUNTIME", "CLIENT_SYNC", "SERVER_RUNTIME",
    "FIRST_RUN_CONNECT", "FIRST_RUN_HOST", "WINDOWS_INSTALLER", "UBUNTU_DEB",
    "FEDORA_RPM", "LINUX_APPIMAGE", "QUICK_INSTALL", "UPGRADE", "REPAIR",
    "UNINSTALL", "DATA_PRESERVATION", "POWER_CYCLE_RECOVERY",
    "ARTIFACT_INTEGRITY", "RELEASE_SIGNING", "TRUST_BOOTSTRAP",
    "DISTRIBUTION_CHANNEL", "DOCUMENTATION",
}
INVENTORY = {
    "windows_installer", "linux_deb", "linux_rpm", "linux_appimage",
    "server_component", "managed_host_dependencies", "release_channel_metadata",
    "release_manifest", "trust_signing_material",
}


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def validate_report(data: dict[str, Any]) -> None:
    require(data.get("schema_version") == 1, "schema_version must be 1")
    require(data.get("report_id") == "P047_RELEASE_CANDIDATE_VALIDATION", "wrong report identity")
    require(data.get("qualification_decision") in RESULTS, "unknown qualification decision")
    require(data.get("current_product_version") == "0.1.0", "current product version must come from the source")
    require(data.get("target_release_version") == "0.2.0", "target release version must remain the planned P048 release")
    require(data.get("release_tag_created") is False, "P047 must not create the v0.2.0 release tag")
    require(isinstance(data.get("p047_pr_merged"), bool), "P047 PR state must be explicit")
    scope = data.get("source_scope")
    require(isinstance(scope, dict), "source_scope is required")
    for field in ("audited_main_sha", "audited_main_tree_sha", "p047_validation_sha", "p047_validation_tree_sha"):
        require(isinstance(scope.get(field), str) and SHA1.fullmatch(scope[field]) is not None,
                f"source_scope.{field} must be an exact full Git SHA")

    inventory = data.get("artifact_inventory")
    require(isinstance(inventory, list), "artifact_inventory is required")
    indexed: dict[str, dict[str, Any]] = {}
    for item in inventory:
        require(isinstance(item, dict), "artifact inventory item must be an object")
        item_id = item.get("inventory_id")
        require(item_id in INVENTORY and item_id not in indexed, f"unknown or duplicate artifact inventory id: {item_id}")
        indexed[item_id] = item
        require(item.get("status") in ARTIFACT_STATES, f"{item_id}: unknown artifact status")
        require(item.get("native_acceptance_status") in RESULTS, f"{item_id}: unknown native acceptance result")
        require(item.get("signature_status") in ARTIFACT_STATES, f"{item_id}: unknown signature status")
        require(item.get("release_candidate_qualification") in RESULTS,
                f"{item_id}: release-candidate qualification must use the acceptance result enum")
        if item["status"] in {"MISSING", "BLOCKED"} and item.get("artifact_id") is None:
            require(item.get("source_sha") is None and item.get("source_tree_sha") is None,
                    f"{item_id}: an absent artifact cannot claim source identities")
        else:
            for field in ("source_sha", "source_tree_sha"):
                require(isinstance(item.get(field), str) and SHA1.fullmatch(item[field]) is not None,
                        f"{item_id}: {field} must be an exact full Git SHA for an existing candidate")
        require(isinstance(item.get("evidence_location"), list), f"{item_id}: evidence_location must be a list")
        if item["status"] == "UNVERIFIED" and item.get("artifact_id") is not None:
            for field in ("artifact_name", "artifact_type", "platform", "architecture", "product_version",
                          "toolchain_identity", "build_workflow_id", "build_job_id", "artifact_id"):
                require(item.get(field) is not None,
                        f"{item_id}: unverified artifact has no {field}")
        if item["status"] == "AVAILABLE":
            for field in ("artifact_name", "artifact_type", "platform", "architecture", "product_version",
                          "toolchain_identity", "build_workflow_id", "build_job_id", "artifact_id", "byte_size", "sha256"):
                require(item.get(field) is not None, f"{item_id}: available artifact has no {field}")
            require(isinstance(item["byte_size"], int) and item["byte_size"] > 0,
                    f"{item_id}: available artifact byte_size must be positive")
            require(isinstance(item["sha256"], str) and SHA256.fullmatch(item["sha256"]) is not None,
                    f"{item_id}: available artifact SHA-256 is invalid")
    require(set(indexed) == INVENTORY, "artifact inventory must include every required producer/dependency")

    rows = data.get("coverage")
    require(isinstance(rows, list), "coverage is required")
    by_id: dict[str, dict[str, Any]] = {}
    for row in rows:
        require(isinstance(row, dict), "coverage row must be an object")
        category = row.get("category")
        require(category in COVERAGE and category not in by_id, f"unknown or duplicate coverage category: {category}")
        by_id[category] = row
        for field in ("requirement", "owning_prompt", "target_platform", "source_revision", "evidence_location", "artifact_identity", "actual_test_result", "evidence_class", "missing_requirements"):
            require(field in row, f"{category}: missing {field}")
        require(isinstance(row["owning_prompt"], list) and row["owning_prompt"], f"{category}: owning_prompt must be nonempty")
        require(isinstance(row["evidence_location"], list), f"{category}: evidence_location must be a list")
        require(isinstance(row["source_revision"], list) and row["source_revision"], f"{category}: source_revision must be nonempty")
        for sha in row["source_revision"]:
            require(isinstance(sha, str) and SHA1.fullmatch(sha) is not None, f"{category}: invalid source revision")
        require(row["evidence_class"] in EVIDENCE_CLASSES, f"{category}: evidence class is not from the acceptance contract")
        require(isinstance(row["missing_requirements"], list), f"{category}: missing_requirements must be a list")
        actual = row["actual_test_result"]
        require(isinstance(actual, dict) and actual.get("result") in RESULTS,
                f"{category}: actual test result must use the acceptance result enum")
        require(isinstance(actual.get("evidence"), list), f"{category}: actual test evidence must be a list")
        for evidence in actual["evidence"]:
            require(evidence.get("result") in RESULTS, f"{category}: evidence result is unknown")
            require(evidence.get("evidence_class") in EVIDENCE_CLASSES, f"{category}: evidence class is unknown")
            require(isinstance(evidence.get("source_revision"), str) and SHA1.fullmatch(evidence["source_revision"]) is not None,
                    f"{category}: evidence source revision is invalid")
        identity = row["artifact_identity"]
        require(isinstance(identity, dict) and identity.get("status") in ARTIFACT_STATES,
                f"{category}: exact artifact identity status is required")
        require(isinstance(identity.get("required"), bool),
                f"{category}: artifact identity applicability must be explicit")
        refs = identity.get("inventory_ids")
        require(isinstance(refs, list) and all(ref in indexed for ref in refs),
                f"{category}: artifact inventory references are invalid")
        require("sha256" in identity and (identity["sha256"] is None or
                (isinstance(identity["sha256"], str) and SHA256.fullmatch(identity["sha256"]) is not None)),
                f"{category}: artifact identity must explicitly record an exact SHA-256 or null")
        require(row.get("qualification_decision") in RESULTS,
                f"{category}: qualification decision must use the acceptance result enum")
        if row["qualification_decision"] == "PASS":
            require(actual["result"] == "PASS", f"{category}: a non-passing test cannot qualify")
            require(not row["missing_requirements"], f"{category}: missing requirements cannot qualify")
            if identity["required"]:
                require(identity["status"] == "AVAILABLE", f"{category}: exact artifact identity is required to qualify")
                require(identity["sha256"] is not None, f"{category}: a qualifying artifact must have an exact SHA-256")
    require(set(by_id) == COVERAGE, "coverage must include every required release gate")

    resume = data.get("live_resume_audit")
    require(isinstance(resume, dict), "live resume audit is required")
    require(isinstance(resume.get("observed_at_utc"), str) and
            re.fullmatch(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z", resume["observed_at_utc"]) is not None,
            "live resume audit must have an exact UTC observation time")
    for field in ("main_sha",):
        require(isinstance(resume.get(field), str) and SHA1.fullmatch(resume[field]) is not None,
                f"live_resume_audit.{field} must be an exact full Git SHA")
    p047 = resume.get("p047")
    require(isinstance(p047, dict) and p047.get("pr_number") == 104 and
            p047.get("state") == "OPEN" and p047.get("draft") is True and
            p047.get("base") == "main" and p047.get("mergeable") is True and
            p047.get("mergeable_state") == "clean",
            "live resume audit must preserve the existing open draft P047 PR")
    for field in ("base_sha", "head_sha_at_snapshot", "validated_product_source_sha"):
        require(isinstance(p047.get(field), str) and SHA1.fullmatch(p047[field]) is not None,
                f"live_resume_audit.p047.{field} must be an exact full Git SHA")
    p045 = resume.get("p045")
    require(isinstance(p045, dict) and p045.get("pr_number") == 78 and
            p045.get("state") == "OPEN" and p045.get("draft") is True and
            p045.get("mergeable") is False and p045.get("mergeable_state") == "dirty",
            "live resume audit must retain the unmerged, non-mergeable P045 PR state")
    for field in ("pr_api_reported_base_sha", "current_main_sha", "head_sha"):
        require(isinstance(p045.get(field), str) and SHA1.fullmatch(p045[field]) is not None,
                f"live_resume_audit.p045.{field} must be an exact full Git SHA")
    conflicts = p045.get("merge_preview_conflicts")
    require(isinstance(conflicts, list) and any(
            isinstance(item, dict) and item.get("path") == "scripts/validate-docs.sh"
            for item in conflicts),
            "live resume audit must preserve the observed P045 merge conflict")
    registry = resume.get("runner_registry_query")
    require(isinstance(registry, dict) and registry.get("result") == "ERROR" and
            registry.get("http_status") == 403 and registry.get("registered_runner_count") is None,
            "inaccessible runner inventory must remain unknown, not be recorded as empty")
    environment = resume.get("execution_environment")
    require(isinstance(environment, dict) and
            environment.get("dev_kvm_present") is False and
            environment.get("windows_11_interactive_session_present") is False and
            environment.get("hypervisor_hard_power_control_present") is False and
            environment.get("qualified_native_worker_accessible") is False,
            "local environment cannot claim P045 native worker capability")
    p036 = resume.get("p036")
    require(isinstance(p036, dict) and p036.get("inventory_present") is False,
            "P036 production inventory must remain absent until supplied")
    p036_run = p036.get("latest_source_workflow_run")
    require(isinstance(p036_run, dict) and p036_run.get("workflow_status") == "queued",
            "P036 workflow status must match the observed live run")
    p036_jobs = {job.get("name"): job for job in p036_run.get("jobs", []) if isinstance(job, dict)}
    require(p036_jobs.get("contract-and-source", {}).get("conclusion") == "success" and
            p036_jobs.get("artifact-identity", {}).get("conclusion") == "failure" and
            all(p036_jobs.get(name, {}).get("status") == "queued"
                for name in ("ubuntu-host", "fedora-host", "interruption-recovery")),
            "P036 source, artifact prerequisite and queued native job states must remain distinct")
    counts = resume.get("qualification_counts")
    actual_counts = {result: sum(row["actual_test_result"]["result"] == result for row in by_id.values())
                     for result in ("PASS", "BLOCKED")}
    qualification_counts = {result: sum(row["qualification_decision"] == result for row in by_id.values())
                            for result in ("PASS", "BLOCKED")}
    require(isinstance(counts, dict) and counts.get("coverage_gate_count") == len(COVERAGE) and
            counts.get("actual_test_result_counts") == actual_counts and
            counts.get("qualification_decision_counts") == qualification_counts,
            "live resume gate counts must exactly match the 21 report rows")
    local_checks = resume.get("local_validation_results")
    require(isinstance(local_checks, list), "live resume audit must record the checks run during this continuation")
    check_results = {item.get("command"): item.get("result") for item in local_checks if isinstance(item, dict)}
    required_checks = {
        "./scripts/validate-p047-release-validation.sh",
        "./scripts/validate-docs.sh",
        "./scripts/validate-install-acceptance.sh",
        "python3 scripts/install_acceptance.py inventory",
        "python3 scripts/validate-installer-security-hardening.py",
        "python3 scripts/validate-installation-resilience-hardening.py",
        "git diff --check",
    }
    require(required_checks.issubset(check_results) and
            all(check_results[command] == "PASS" for command in required_checks),
            "live resume validation commands must be present with their observed results")
    release_state = resume.get("release_state")
    require(isinstance(release_state, dict) and
            all(release_state.get(field) is False for field in (
                "authenticated_v020_candidate_set_complete",
                "current_d712_raw_payload_sha256_independently_verified",
                "production_signing_ready", "trusted_bootstrap_ready",
                "authenticated_distribution_channel_ready", "p047_merged", "p048_ready")),
            "live resume audit must keep unavailable release dependencies blocked")
    scenarios = data.get("main_product_audit", {}).get("acceptance_inventory", {})
    require(scenarios.get("requested_definition_file", {}).get("status") == "MISSING" and
            scenarios.get("requested_definition_file", {}).get("path") == "acceptance/install-scenarios-v1.json" and
            scenarios.get("evidence_location", "").startswith("scripts/install_acceptance.py inventory"),
            "scenario inventory must identify the actual command and missing requested JSON path")

    if data["qualification_decision"] == "PASS":
        require(all(row["qualification_decision"] == "PASS" for row in by_id.values()),
                "overall PASS requires every release gate to pass")
        require(not data.get("release_blockers"), "overall PASS cannot retain release blockers")
        require(all(item["status"] == "AVAILABLE" for item in indexed.values()),
                "overall PASS requires a complete exact artifact inventory")
        require(all(item["release_candidate_qualification"] == "PASS" for item in indexed.values()),
                "overall PASS requires every artifact to qualify for the target release")
        require(all(item["signature_status"] == "AVAILABLE" for item in indexed.values()),
                "overall PASS requires verified production signature status for every artifact")
        dependencies = data.get("release_dependencies", {})
        require(dependencies.get("p045", {}).get("accepted_and_merged") is True,
                "overall PASS requires accepted P045 evidence")
        require(dependencies.get("p036", {}).get("production_acceptance") == "PASS",
                "overall PASS requires accepted P036 production evidence")
        require(dependencies.get("production_signing", {}).get("ready") is True,
                "overall PASS requires production signing and trust readiness")
        require(data["p047_pr_merged"] is True, "overall PASS requires accepted P047 work merged to main")


def main() -> int:
    data = json.loads(REPORT.read_text(encoding="utf-8"))
    validate_report(data)
    print(f"P047 report contract: PASS ({len(data['coverage'])} gates, decision {data['qualification_decision']})")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, UnicodeError, json.JSONDecodeError, ValueError) as error:
        print(f"P047 report contract: FAIL: {error}", file=sys.stderr)
        raise SystemExit(1)
