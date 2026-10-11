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
