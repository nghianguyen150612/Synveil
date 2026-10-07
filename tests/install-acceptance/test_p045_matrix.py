"""Adversarial evidence-gate tests. Synthetic PASS records stay in this fixture."""
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import sys
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
spec = importlib.util.spec_from_file_location("p045", ROOT / "scripts/validate-cross-platform-clean-machine-matrix.py")
p045 = importlib.util.module_from_spec(spec)
spec.loader.exec_module(p045)


class MatrixGateTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.artifacts = Path(self.tmp.name)
        self.source = "a" * 40
        self.row = next(r for r in p045.expected_rows() if r["id"] == "ubuntu-24.04-x86_64-deb")
        self.scenario = p045.definitions()["INSTALL-JOURNEY-2"]
        self.record = p045.contract.blocked_result(self.scenario)
        payload = b"synthetic package fixture, never native acceptance evidence"
        (self.artifacts / "fixture.deb").write_bytes(payload)
        digest = hashlib.sha256(payload).hexdigest()
        manifest = {"source_commit": self.source, "product_version": "0.1.0", "artifacts": [
            {"id": "fixture-deb", "filename": "fixture.deb", "sha256": digest, "size_bytes": len(payload), "artifact_type": "deb"}]}
        path = self.artifacts / "SYNVEIL-RELEASE-MANIFEST.json"
        path.write_text(json.dumps(manifest))
        self.record.update(result="PASS", evidence_classification="native-clean-machine", reason=None,
                           completed_steps=[s["id"] for s in self.scenario["steps"]],
                           assertion_results=[{"assertion_id": a["id"], "result": "PASS"} for a in self.scenario["assertions"]],
                           capability_results=[{"name": c, "status": "available"} for c in p045.contract.required_capabilities_for_host(self.scenario, {"family": "ubuntu"})],
                           cleanup_result={"status": "complete", "details": "fixture"})
        self.record["platform_facts"].update(family="ubuntu", os_id="ubuntu", version="24.04", architecture="x86_64",
                                             desktop_session="available", source_commit=self.source,
                                             image_sha256=p045.image_digest("ubuntu", "24.04"), machine_identity="consumer-vm",
                                             workflow_run_id="1", job_id="2", producer_run_id="1", producer_job_id="3",
                                             cleanliness_probe="clean", preservation_verified=True, reproducibility_verified=True, gui_evidence=["fixture.png"],
                                             session_method="interactive", execution_method="native-installed-product", artifact_id="fixture-deb")
        self.record["artifact_identity"].update(status="verified", source_commit=self.source, artifact_type="deb",
                                               filename="fixture.deb", digest=digest, sha256=digest, size_bytes=len(payload),
                                               platform="linux", architecture="x86_64", product_version="0.1.0",
                                               manifest_identity={"source_commit": self.source, "product_version": "0.1.0", "manifest_sha256": hashlib.sha256(path.read_bytes()).hexdigest()})

    def validate(self, record=None):
        p045.validate_record(record or self.record, self.row, self.source, self.artifacts)

    def test_complete_synthetic_shape_and_manifest_binding(self):
        self.validate()

    def test_weaker_evidence_is_rejected(self):
        for evidence in ("fixture", "static", "ci-native-scoped", "contract-valid"):
            with self.subTest(evidence=evidence), self.assertRaises(ValueError):
                record = copy.deepcopy(self.record)
                record["evidence_classification"] = evidence
                self.validate(record)

    def test_promoting_fixture_or_offscreen_label_is_rejected(self):
        for field, value in (("execution_method", "fixture"), ("session_method", "offscreen"),
                             ("session_method", "service"), ("session_method", "silent")):
            with self.subTest(field=field, value=value), self.assertRaises(ValueError):
                record = copy.deepcopy(self.record)
                record["platform_facts"][field] = value
                self.validate(record)

    def test_stale_runner_candidate_and_scenario_are_rejected(self):
        for container, field in (("artifact_identity", "source_commit"), ("platform_facts", "source_commit")):
            with self.subTest(container=container), self.assertRaises(ValueError):
                record = copy.deepcopy(self.record)
                record[container][field] = "b" * 40
                self.validate(record)
        self.record["scenario_definition_digest"] = "0" * 64
        with self.assertRaises(ValueError):
            self.validate()

    def test_debian_wrong_version_and_unpinned_image_are_rejected(self):
        for field, value in (("os_id", "debian"), ("version", "22.04"), ("image_sha256", "0" * 64)):
            with self.subTest(field=field), self.assertRaises(ValueError):
                record = copy.deepcopy(self.record)
                record["platform_facts"][field] = value
                self.validate(record)

    def test_missing_steps_assertions_and_identity_are_rejected(self):
        for field in ("completed_steps", "assertion_results"):
            with self.subTest(field=field), self.assertRaises(ValueError):
                record = copy.deepcopy(self.record)
                record[field] = []
                self.validate(record)
        self.record["artifact_identity"]["filename"] = None
        with self.assertRaises(ValueError):
            self.validate()

    def test_tampered_candidate_and_manifest_are_rejected(self):
        (self.artifacts / "fixture.deb").write_bytes(b"tampered")
        with self.assertRaises(ValueError):
            self.validate()

    def test_unknown_schema_fields_are_rejected(self):
        self.record["intended_pass"] = True
        with self.assertRaises(ValueError):
            self.validate()

    def test_sigkill_cannot_qualify_power_cycle(self):
        scenario = p045.definitions()["INSTALL-JOURNEY-8"]
        self.record.update(scenario_id=scenario["id"], scenario_definition_digest=p045.contract.definition_digest(scenario),
                           execution_class=scenario["execution_class"], evidence_classification="interruption-power-cycle",
                           completed_steps=[s["id"] for s in scenario["steps"]],
                           assertion_results=[{"assertion_id": a["id"], "result": "PASS"} for a in scenario["assertions"]])
        self.record["platform_facts"].update(interruption_method="SIGKILL", reboot_verified=True,
                                             interruption_points_verified=scenario["interruption_points"])
        with self.assertRaisesRegex(ValueError, "power cuts"):
            self.validate()

    def test_missing_native_results_create_only_blocked_preflight(self):
        report = p045.aggregate(Path(self.tmp.name), self.source)
        self.assertFalse(p045.validate_report(report, self.source))
        self.assertTrue(all(e["record"]["result"] == "BLOCKED" and
                            e["record"]["evidence_classification"] == "contract-valid" for e in report["records"]))
        report["records"].pop()
        with self.assertRaisesRegex(ValueError, "missing required"):
            p045.validate_report(report, self.source)

    def test_debian_qualification_and_p046_are_rejected(self):
        matrix = json.loads(p045.MATRIX.read_text())
        matrix["unsupported"]["debian"] = "qualified"
        with self.assertRaises(ValueError):
            p045.validate_matrix(matrix)

    def test_windows_system_api_does_not_exempt_vc_runtime(self):
        # Execute the reviewed classifier alone, never the package builder.
        source = (ROOT / "deploy/packages/build-windows.sh").read_text()
        begin = source.index("is_system_dll() {")
        end = source.index("\n}\n", begin) + 3
        classifier = source[begin:end]
        command = classifier + "\nis_system_dll UIAutomationCore.DLL && ! is_system_dll MSVCP140.dll && ! is_system_dll VCRUNTIME140.dll && ! is_system_dll unrelated.dll\n"
        result = subprocess.run(["bash", "-c", command], capture_output=True, text=True, timeout=5)
        self.assertEqual(result.returncode, 0, result.stderr)
        matrix = json.loads(p045.MATRIX.read_text())
        matrix["p046"] = "implemented"
        with self.assertRaises(ValueError):
            p045.validate_matrix(matrix)


if __name__ == "__main__":
    unittest.main()
