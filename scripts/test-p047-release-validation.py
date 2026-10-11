#!/usr/bin/env python3
"""Focused tests for the P047 final-evidence fail-closed validator."""
import importlib.util
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "validate_p047", ROOT / "scripts/validate-p047-release-validation.py"
)
VALIDATOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VALIDATOR)
REPORT = ROOT / "acceptance/p047/release-candidate-validation-v1.json"


class P047ReportTests(unittest.TestCase):
    def setUp(self):
        # The committed report is the fail-closed fixture; mutation happens only
        # in memory and is never written to the acceptance record.
        import json
        self.data = json.loads(REPORT.read_text(encoding="utf-8"))

    def test_complete_blocked_inventory_is_valid(self):
        VALIDATOR.validate_report(self.data)
        self.assertEqual(self.data["qualification_decision"], "BLOCKED")

    def test_unknown_result_enum_is_rejected(self):
        self.data["coverage"][0]["actual_test_result"]["result"] = "INCOMPLETE"
        with self.assertRaises(ValueError):
            VALIDATOR.validate_report(self.data)

    def test_required_coverage_category_cannot_be_omitted(self):
        self.data["coverage"].pop()
        with self.assertRaises(ValueError):
            VALIDATOR.validate_report(self.data)

    def test_missing_artifact_cannot_be_promoted_to_pass(self):
        row = next(x for x in self.data["coverage"] if x["category"] == "WINDOWS_INSTALLER")
        row["actual_test_result"]["result"] = "PASS"
        row["qualification_decision"] = "PASS"
        row["missing_requirements"] = []
        with self.assertRaises(ValueError):
            VALIDATOR.validate_report(self.data)

    def test_absent_artifact_cannot_claim_source_identity(self):
        item = next(x for x in self.data["artifact_inventory"] if x["inventory_id"] == "linux_deb")
        item["source_sha"] = self.data["source_scope"]["p047_validation_sha"]
        item["source_tree_sha"] = self.data["source_scope"]["p047_validation_tree_sha"]
        with self.assertRaises(ValueError):
            VALIDATOR.validate_report(self.data)

    def test_available_artifact_requires_exact_identity(self):
        item = next(x for x in self.data["artifact_inventory"] if x["inventory_id"] == "windows_installer")
        item["status"] = "AVAILABLE"
        with self.assertRaises(ValueError):
            VALIDATOR.validate_report(self.data)

    def test_qualifying_coverage_requires_exact_artifact_sha256(self):
        row = next(x for x in self.data["coverage"] if x["category"] == "LINUX_APPIMAGE")
        row["actual_test_result"]["result"] = "PASS"
        row["qualification_decision"] = "PASS"
        row["missing_requirements"] = []
        row["artifact_identity"]["status"] = "AVAILABLE"
        row["artifact_identity"]["sha256"] = None
        with self.assertRaises(ValueError):
            VALIDATOR.validate_report(self.data)

    def test_source_quality_can_qualify_without_a_product_artifact(self):
        row = next(x for x in self.data["coverage"] if x["category"] == "SOURCE_QUALITY")
        row["actual_test_result"]["result"] = "PASS"
        row["qualification_decision"] = "PASS"
        row["missing_requirements"] = []
        row["artifact_identity"].update(required=False, status="UNVERIFIED", sha256=None, inventory_ids=[])
        VALIDATOR.validate_report(self.data)

    def test_global_pass_requires_accepted_external_gates(self):
        self.data["qualification_decision"] = "PASS"
        self.data["release_blockers"] = []
        for row in self.data["coverage"]:
            row["actual_test_result"]["result"] = "PASS"
            row["qualification_decision"] = "PASS"
            row["missing_requirements"] = []
            row["artifact_identity"]["status"] = "AVAILABLE"
            row["artifact_identity"]["sha256"] = "a" * 64
        with self.assertRaises(ValueError):
            VALIDATOR.validate_report(self.data)

    def test_unsigned_ci_artifact_does_not_qualify_for_release(self):
        item = next(x for x in self.data["artifact_inventory"] if x["inventory_id"] == "linux_appimage")
        item.update(status="AVAILABLE", release_candidate_qualification="BLOCKED",
                    artifact_name="Synveil-0.1.0-x86_64.AppImage", artifact_type="appimage",
                    platform="Linux x86_64", architecture="x86_64", product_version="0.1.0",
                    toolchain_identity="CI fixture", build_workflow_id=1, build_job_id=2,
                    artifact_id=3, byte_size=4, sha256="a" * 64, source_sha="a" * 40,
                    source_tree_sha="b" * 40)
        VALIDATOR.validate_report(self.data)


if __name__ == "__main__":
    unittest.main(verbosity=2)
