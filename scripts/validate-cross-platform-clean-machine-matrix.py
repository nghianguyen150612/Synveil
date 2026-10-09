#!/usr/bin/env python3
"""Validate P045 structure and aggregate evidence without promoting missing gates."""
from __future__ import annotations

import argparse
from datetime import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import sys

import install_acceptance as contract

ROOT = Path(__file__).resolve().parents[1]
MATRIX = ROOT / "deploy/acceptance/p045-matrix-v1.json"
SCHEMA = ROOT / "tests/install-acceptance/schema/result-v1.schema.json"


def require(value, reason):
    if not value:
        raise ValueError(reason)


def validate_schema(value, schema, location="result"):
    """Evaluate the keywords used by the authoritative, reference-free schema."""
    kinds = {"object": dict, "array": list, "string": str, "integer": int,
             "null": type(None), "boolean": bool}
    if "type" in schema:
        names = schema["type"] if isinstance(schema["type"], list) else [schema["type"]]
        require(any(type(value) is kinds[name] for name in names), f"{location}: invalid type")
    if "const" in schema:
        require(value == schema["const"], f"{location}: invalid constant")
    if "enum" in schema:
        require(value in schema["enum"], f"{location}: invalid enum")
    if isinstance(value, dict):
        require(set(schema.get("required", [])) <= value.keys(), f"{location}: missing fields")
        properties = schema.get("properties", {})
        if schema.get("additionalProperties") is False:
            require(set(value) <= properties.keys(), f"{location}: unknown fields")
        for key, item in value.items():
            if key in properties:
                validate_schema(item, properties[key], f"{location}.{key}")
    if isinstance(value, list) and "items" in schema:
        for item in value:
            validate_schema(item, schema["items"], f"{location}[]")
    if isinstance(value, str):
        if "pattern" in schema:
            require(re.search(schema["pattern"], value), f"{location}: invalid pattern")
        if schema.get("format") == "date-time":
            require(datetime.fromisoformat(value.replace("Z", "+00:00")).tzinfo is not None,
                    f"{location}: time must include offset")


def definitions():
    return {s["id"]: s for _, s in contract.discover()}


def expected_rows():
    policy = json.loads((ROOT / "deploy/install/linux-platforms-v1.json").read_text())
    require({(t["os_id"], t["version_id"], t["architecture"], t["artifact_type"])
             for t in policy["targets"]} == {("ubuntu", "24.04", "x86_64", "deb"), ("fedora", "42", "x86_64", "rpm")}, "P045 policy must not broaden exact qualified Linux targets")
    common = ["INSTALL-JOURNEY-6", "INSTALL-JOURNEY-7", "INSTALL-JOURNEY-8", "FIRST-RUN-1", "FIRST-RUN-2"]
    rows = [{"id": "windows-11-amd64-setup", "os_id": "windows", "version": "11",
             "architecture": "x86_64", "artifact_type": "windows_installer",
             "scenarios": ["INSTALL-JOURNEY-1", *common]}]
    for target in policy["targets"]:
        for route, primary in ((target["artifact_type"], "INSTALL-JOURNEY-2" if target["os_id"] == "ubuntu" else "INSTALL-JOURNEY-3"),
                               ("appimage", "INSTALL-JOURNEY-4"), ("quick-install", "INSTALL-JOURNEY-5")):
            rows.append({"id": f"{target['id']}-{route}", "os_id": target["os_id"],
                         "version": target["version_id"], "architecture": target["architecture"],
                         "artifact_type": target["artifact_type"] if route == "quick-install" else route,
                         "scenarios": [primary] if route == "quick-install" else [primary, *common]})
    return rows


def validate_matrix(matrix):
    require(matrix["schema_version"] == 1, "matrix schema must be v1")
    require(matrix["rows"] == expected_rows(), "required rows must match current Linux policy and Windows 11")
    require(matrix["unsupported"]["debian"] == "detected-not-qualified", "Debian is not qualified")
    require(matrix["unsupported"]["other-linux"] == "not-qualified", "no universal Linux qualification")
    scenarios = definitions()
    for row in matrix["rows"]:
        for sid in row["scenarios"]:
            require(sid in scenarios, f"invalid scenario: {sid}")
            family = "linux_generic" if row["artifact_type"] == "appimage" else row["os_id"]
            require(family in scenarios[sid]["platform"]["applicable_families"], f"invalid applicability: {row['id']}/{sid}")
    require(matrix["status"] == "acceptance-withheld", "completion needs observed final-head evidence and merge")
    require(matrix["p046"] == "deferred" and matrix["release"] == "not-created", "release boundary violated")


def image_digest(os_id, version):
    name = f"{os_id}-{version}-x86_64"
    for line in (ROOT / "deploy/acceptance/images.lock").read_text().splitlines():
        fields = line.split()
        if fields and fields[0] == name:
            return next(x.removeprefix("sha256:") for x in fields[1:] if x.startswith("sha256:"))
    raise ValueError(f"missing pinned image: {name}")


def validate_record(record, row, source_commit, artifact_root=None):
    validate_schema(record, json.loads(SCHEMA.read_text()))
    contract.validate_result_record(record)
    scenario = definitions()[record["scenario_id"]]
    require(record["scenario_id"] in row["scenarios"], "scenario is not required by this row")
    require(record["scenario_definition_digest"] == contract.definition_digest(scenario), "stale scenario digest")
    require(record["execution_class"] == scenario["execution_class"], "wrong execution class")
    if record["result"] != "PASS":
        require(bool(record["reason"]), "non-PASS must explain its blocker/failure")
        return
    evidence = record["evidence_classification"]
    require(evidence in scenario["evidence_requirements"]["satisfies"], "evidence does not satisfy this scenario")
    require(record["start_time"] and record["end_time"], "PASS needs execution times")
    require(datetime.fromisoformat(record["end_time"]) >= datetime.fromisoformat(record["start_time"]), "invalid time order")
    require(record["completed_steps"] == [s["id"] for s in scenario["steps"]], "incomplete journey")
    assertions = record["assertion_results"]
    required_assertions = {s["id"] for s in scenario["assertions"]}
    require({s["assertion_id"] for s in assertions} >= required_assertions and
            all(s["result"] == "PASS" for s in assertions), "missing/failed assertions")
    facts = record["platform_facts"]
    require(facts.get("execution_method") == "native-installed-product", "fixture/source/offscreen execution cannot qualify native acceptance")
    require(facts.get("source_commit") == source_commit, "superseded runner source")
    require(facts.get("os_id", facts["family"]) == row["os_id"] and
            facts["architecture"] == row["architecture"], "wrong tested platform")
    if row["os_id"] == "windows":
        require(facts.get("windows_product") == "Windows 11" and
                re.fullmatch(r"\d+(?:\.\d+)*", facts.get("os_build", "")) and
                facts.get("edition") and facts.get("version"), "Windows 11 identity/build must be observed")
    else:
        require(facts["version"] == row["version"], "wrong exact Linux version")
        require(facts.get("image_sha256") == image_digest(row["os_id"], row["version"]), "unpinned machine image")
    for field in ("machine_identity", "workflow_run_id", "job_id", "producer_run_id"):
        require(bool(facts.get(field)), f"missing native provenance: {field}")
    require(facts.get("producer_job_id") and facts["producer_job_id"] != facts["job_id"], "producer and consumer must be distinct jobs/machines")
    require(facts.get("cleanliness_probe") in ({"clean", "declared-prior-install"} if record["scenario_id"] in {"INSTALL-JOURNEY-6", "INSTALL-JOURNEY-7"} else {"clean"}), "missing cleanliness probe")
    require(facts.get("preservation_verified") is True, "missing preservation evidence")
    require(facts.get("reproducibility_verified") is True, "candidate reproducibility gate not established")
    require(record["cleanup_result"]["status"] == "complete", "cleanup must be observed separately")
    if "graphical_session" in scenario["required_capabilities"]:
        require(facts["desktop_session"] == "available" and facts.get("gui_evidence"), "missing graphical evidence")
        require(facts.get("session_method") not in {"offscreen", "service", "silent"}, "invalid graphical session")
    if record["scenario_id"] == "INSTALL-JOURNEY-8":
        require(facts.get("interruption_method") == "hard-vm-power-cut" and facts.get("reboot_verified") is True and
                set(facts.get("interruption_points_verified", [])) == set(scenario["interruption_points"]), "process kill/reboot does not prove required power cuts")
    required_caps = contract.required_capabilities_for_host(scenario, facts)
    available = {c["name"] for c in record["capability_results"] if c["status"] == "available"}
    require(set(required_caps) <= available, "required capability missing")
    artifact = record["artifact_identity"]
    require(artifact["source_commit"] == source_commit, "superseded candidate source")
    require(artifact["status"] == "verified" and artifact["artifact_type"].lower() == row["artifact_type"] and
            artifact["sha256"] and artifact["size_bytes"] and artifact["product_version"] and artifact["filename"], "incomplete candidate identity")
    manifest = artifact["manifest_identity"]
    require(manifest and manifest["source_commit"] == source_commit and manifest["manifest_sha256"], "missing manifest identity")
    require(artifact["architecture"] == row["architecture"] and artifact["platform"] in {row["os_id"], "windows" if row["os_id"] == "windows" else "linux"}, "artifact platform/architecture mismatch")
    if record["scenario_id"] == "INSTALL-JOURNEY-5":
        require(artifact["trust_status"] == "authenticated" and facts.get("trust_chain_verified") is True, "quick install trust chain not established")
    require(artifact_root is not None, "PASS requires exact producer bytes for verification")
    filename = artifact["filename"]
    require(Path(filename).name == filename and filename not in {".", ".."}, "unsafe candidate filename")
    candidates = list(artifact_root.rglob(filename))
    require(len(candidates) == 1, "candidate bytes missing or ambiguous")
    payload = candidates[0]
    with payload.open("rb") as stream:
        require(payload.stat().st_size == artifact["size_bytes"] and hashlib.file_digest(stream, "sha256").hexdigest() == artifact["sha256"], "candidate bytes changed")
    manifests = list(artifact_root.rglob("SYNVEIL-RELEASE-MANIFEST.json"))
    matched = []
    for path in manifests:
        if hashlib.sha256(path.read_bytes()).hexdigest() == manifest["manifest_sha256"]:
            document = json.loads(path.read_text(encoding="utf-8-sig"))
            require(document["source_commit"] == source_commit and document["product_version"] == artifact["product_version"], "manifest source/version mismatch")
            matched.extend(a for a in document["artifacts"] if a.get("id") == facts.get("artifact_id") and a["filename"] == filename and a["sha256"] == artifact["sha256"] and a["size_bytes"] == artifact["size_bytes"] and a["artifact_type"].lower() == row["artifact_type"])
    require(len(matched) == 1, "artifact ID/bytes not bound to exact producer manifest")


def validate_report(report, source_commit, artifact_root=None):
    require(report["source_commit"] == source_commit, "report source is superseded")
    expected = {(row["id"], sid): row for row in expected_rows() for sid in row["scenarios"]}
    seen = set()
    for entry in report["records"]:
        key = (entry["row_id"], entry["record"]["scenario_id"])
        require(key in expected and key not in seen, f"invalid/duplicate matrix result: {key}")
        seen.add(key)
        validate_record(entry["record"], expected[key], source_commit, artifact_root)
    require(seen == set(expected), "missing required matrix results")
    source_gates = report.get("source_gates")
    complete = bool(source_gates and source_gates.get("status") == "PASS" and source_gates.get("source_commit") == source_commit and
                    source_gates.get("workflow_run_id") and set(source_gates.get("gates", [])) == {"format", "strict-workspace-clippy", "cargo-deny"}) and all(e["record"]["result"] == "PASS" for e in report["records"])
    errors = report.get("input_errors", [])
    require(report["result"] == ("ERROR" if errors else "PASS" if complete else "BLOCKED"), "aggregate conceals non-PASS evidence")
    return complete and not errors


def aggregate(directory, source_commit):
    raw = []
    candidates = []
    errors = []
    source_gates = None
    for path in sorted(directory.rglob("*.json")):
        try:
            item = json.loads(path.read_text(encoding="utf-8-sig"))
            if isinstance(item, dict) and "scenario_id" in item:
                validate_schema(item, json.loads(SCHEMA.read_text()))
                raw.append(item)
            if path.name == "p045-source-gates.json":
                require(source_gates is None and item["source_commit"] == source_commit, "ambiguous/stale source gates")
                source_gates = item
            if path.name == "SYNVEIL-RELEASE-MANIFEST.json":
                require(item["source_commit"] == source_commit, "producer manifest source mismatch")
                for artifact in item["artifacts"]:
                    require(Path(artifact["filename"]).name == artifact["filename"], "unsafe producer filename")
                    payload = path.parent / artifact["filename"]
                    with payload.open("rb") as stream:
                        require(payload.stat().st_size == artifact["size_bytes"] and hashlib.file_digest(stream, "sha256").hexdigest() == artifact["sha256"], "producer bytes mismatch")
                    candidates.append({**artifact, "source_commit": item["source_commit"],
                                       "product_version": item["product_version"], "manifest_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                                       "producer_manifest": str(path.relative_to(directory))})
        except (ValueError, KeyError, OSError, TypeError) as exc:
            errors.append(f"{path.relative_to(directory)}: {exc}")
    records = []
    for row in expected_rows():
        for sid in row["scenarios"]:
            found = [r for r in raw if r["scenario_id"] == sid and r["platform_facts"].get("os_id", r["platform_facts"].get("family")) == row["os_id"] and
                     (row["os_id"] == "windows" or r["platform_facts"].get("version") == row["version"]) and
                     r["artifact_identity"]["artifact_type"].lower() == row["artifact_type"]]
            require(len(found) <= 1, f"ambiguous evidence for {row['id']}/{sid}; do not select the green retry")
            if found:
                record = found[0]
            else:
                record = contract.blocked_result(definitions()[sid])
                record.update(result="BLOCKED", runner_version=f"p045-aggregate@{source_commit}",
                              reason=f"No qualifying consumer result for {row['id']}/{sid}. Aggregate preflight only; required native product journey has not been established.")
            records.append({"row_id": row["id"], "record": record})
    return {"schema_version": 1, "source_commit": source_commit, "workflow_run_id": os.environ.get("GITHUB_RUN_ID"),
            "result": "ERROR" if errors else "PASS" if source_gates and source_gates.get("status") == "PASS" and all(r["record"]["result"] == "PASS" for r in records) else "BLOCKED",
            "source_gates": source_gates, "candidates": candidates, "input_errors": errors, "records": records}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--aggregate", type=Path)
    parser.add_argument("--report", type=Path)
    parser.add_argument("--source-commit")
    parser.add_argument("--artifact-root", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--require-pass", action="store_true")
    args = parser.parse_args()
    validate_matrix(json.loads(MATRIX.read_text()))
    if args.aggregate or args.report:
        require(args.source_commit and re.fullmatch(r"[0-9a-f]{40}", args.source_commit), "exact source commit required")
        report = aggregate(args.aggregate, args.source_commit) if args.aggregate else json.loads(args.report.read_text())
        if args.output:
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
        complete = validate_report(report, args.source_commit, args.artifact_root)
        print(f"P045 matrix: {report['result']} ({len(report['records'])} required row/scenario records)")
        return 0 if complete or not args.require_pass else 1
    docs = (ROOT / "docs/v0.2/CROSS_PLATFORM_CLEAN_MACHINE_MATRIX.md").read_text()
    manifest = (ROOT / "docs/v0.2/PROMPT045_MANIFEST.md").read_text()
    roadmap = (ROOT / "docs/v0.2/ROADMAP.md").read_text().split("### P045", 1)[1].split("### P046", 1)[0]
    for content in (docs, manifest, roadmap):
        require("acceptance" in content.lower() and "withheld" in content.lower(), "P045 status must withhold acceptance")
        require(not re.search(r"(?:P045_READY_FOR_P046|SYNVEIL_V0_2_0_RELEASED)", content), "premature completion/release claim")
    for row in expected_rows():
        require(row["id"] in docs, f"missing documented row: {row['id']}")
    require("detected-not-qualified" in docs and "P046 deferred" in manifest, "missing policy/release boundary")
    require(not (ROOT / "docs/v0.2/PROMPT046_MANIFEST.md").exists(), "P046 must not begin")
    print("P045 structure valid; acceptance withheld; no native evidence manufactured")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (ValueError, KeyError, TypeError, OSError, contract.ContractError) as exc:
        print(f"P045 validation error: {exc}", file=sys.stderr)
        raise SystemExit(2)
