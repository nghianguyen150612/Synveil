"""Failure-path and provenance fixtures; never qualifying native evidence."""
import hashlib
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
import linux_acceptance as linux


class LinuxResultBoundaryTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        payload = b"fixture bytes, never native product evidence"
        (self.root / "fixture.deb").write_bytes(payload)
        self.manifest = self.root / "SYNVEIL-RELEASE-MANIFEST.json"
        self.manifest.write_text(json.dumps({"source_commit": "a" * 40, "product_version": "0.1.0", "artifacts": [
            {"filename": "fixture.deb", "artifact_type": "deb", "platform": "linux", "architecture": "x86_64",
             "size_bytes": len(payload), "sha256": hashlib.sha256(payload).hexdigest()}]}))
        self.facts = linux.HostFacts("ubuntu", "24.04", (), "x86_64", "6.8", "x11", "available", "apt-get", None)
        self.scenarios = {s["id"]: (p, s) for p, s in linux.contract.discover()}
        for name, replacement in (("collect_host_facts", lambda: self.facts),
                                  ("detect_capability", lambda *_: "available"),
                                  ("probe_clean_machine", lambda *_: linux.CleanlinessReport())):
            mock = patch.object(linux, name, replacement)
            mock.start()
            self.addCleanup(mock.stop)

    def execute(self, scenario="INSTALL-JOURNEY-7"):
        path, definition = self.scenarios[scenario]
        record = linux.execute(definition, path, manifest=self.manifest, artifact_type="deb", evidence="native-clean-machine")
        linux.contract.validate_result_record(json.loads(json.dumps(record)))
        self.assertLess(len(json.dumps(record)), 16384)
        return record

    def test_absent_baseline_cannot_remove_or_repair_clean_guest(self):
        for sid in ("INSTALL-JOURNEY-6", "INSTALL-JOURNEY-7"):
            with self.subTest(sid=sid), patch.object(linux.Adapter, "_package_installed", return_value=False), patch.object(linux.Adapter, "dispatch") as action:
                record = self.execute(sid)
                self.assertEqual(record["result"], "BLOCKED")
                self.assertIn("installed baseline", record["reason"])
                action.assert_not_called()

    def test_missing_tool_serializes_blocked_and_redacts_credentials(self):
        with patch.object(linux.Adapter, "_package_installed", return_value=True), patch.object(linux.Adapter, "dispatch", side_effect=linux.MissingCapability("required tool not present: xdotool password=hidden")):
            record = self.execute()
        self.assertEqual(record["result"], "BLOCKED")
        self.assertIn("xdotool", record["reason"])
        self.assertNotIn("hidden", json.dumps(record))

    def test_timeout_and_os_failure_serialize_error_and_attempt_cleanup(self):
        for error in (linux.AdapterError("command exceeded its bound"), OSError("fixture failure")):
            with patch.object(linux.Adapter, "_package_installed", return_value=True), patch.object(linux.Adapter, "dispatch", side_effect=error), patch.object(linux.Adapter, "cleanup", return_value={"status": "complete", "details": None}) as cleanup:
                self.assertEqual(self.execute()["result"], "ERROR")
                cleanup.assert_called_once()

    def test_local_action_completion_does_not_promote_missing_provenance(self):
        def action(adapter, step):
            adapter.observations["package_removed"] = True
            return linux.StepOutcome(step["id"], step["action"], "completed")
        with patch.object(linux.Adapter, "_package_installed", return_value=True), patch.object(linux.Adapter, "dispatch", action):
            record = self.execute()
        self.assertEqual(record["assertion_results"][0]["result"], "PASS")
        self.assertEqual(record["result"], "BLOCKED")
        self.assertIn("provenance", record["reason"])
        self.assertIn("preservation", record["reason"])

    def test_preflight_exception_still_serializes_valid_bounded_error(self):
        with patch.object(linux, "detect_capability", side_effect=ValueError("invalid preflight")):
            self.assertEqual(self.execute()["result"], "ERROR")

    def test_appimage_never_uses_native_package_removal(self):
        identity = linux.ArtifactIdentity("fixture.AppImage", "0" * 64, 1, "appimage", "0.1.0", "a" * 40, "linux", "x86_64", None, "fixture", None)
        adapter = linux.Adapter(self.facts, identity, evidence="fixture")
        with patch.object(adapter, "_sudo") as sudo:
            for action in ("repair", "uninstall"):
                self.assertEqual(adapter.dispatch({"id": "S", "action": action}).status, "blocked")
            sudo.assert_not_called()

    def test_display_environment_and_offscreen_cannot_establish_native_session(self):
        with patch.dict(linux.os.environ, {"DISPLAY": ":0", "QT_QPA_PLATFORM": "offscreen"}), patch.object(linux, "_run") as command:
            self.assertFalse(linux.graphical_session_available())
            command.assert_not_called()
        with patch.dict(linux.os.environ, {"DISPLAY": ":0", "QT_QPA_PLATFORM": "xcb"}), patch.object(linux.shutil, "which", return_value="/fixture/tool"), patch.object(linux, "_run", return_value=linux.subprocess.CompletedProcess([], 0, "", "")):
            self.assertFalse(linux.graphical_session_available())

    def test_observed_assertion_failure_is_distinct_from_missing_observation(self):
        def action(adapter, step):
            return linux.StepOutcome(step["id"], step["action"], "completed")
        with patch.object(linux.Adapter, "_package_installed", return_value=True), patch.object(linux.Adapter, "dispatch", action):
            record = self.execute()
        self.assertEqual(record["result"], "BLOCKED")
        self.assertEqual(record["assertion_results"][0]["result"], "NOT_EVALUATED")
        def failed_action(adapter, step):
            adapter.observations["package_removed"] = False
            return action(adapter, step)
        with patch.object(linux.Adapter, "_package_installed", return_value=True), patch.object(linux.Adapter, "dispatch", failed_action):
            record = self.execute()
        self.assertEqual(record["result"], "FAIL")
        self.assertEqual(record["assertion_results"][0]["result"], "FAIL")

    def test_transport_failure_cli_emits_valid_error_with_unknown_cleanup(self):
        image = "ubuntu-24.04-x86_64"
        digest = next(f.removeprefix("sha256:") for line in (ROOT / "deploy/acceptance/images.lock").read_text().splitlines()
                      if line.split() and line.split()[0] == image for f in line.split()[1:] if f.startswith("sha256:"))
        process = linux.subprocess.run([sys.executable, str(ROOT / "scripts/linux_acceptance.py"), "error-guest", "INSTALL-JOURNEY-7",
                                       "--manifest", str(self.manifest), "--artifact-type", "deb", "--os-id", "ubuntu", "--version", "24.04",
                                       "--image-name", image, "--image-sha256", digest, "--source-commit", "a" * 40,
                                       "--reason", "SSH timeout; product outcome unknown"], capture_output=True, text=True, timeout=10)
        self.assertEqual(process.returncode, 0, process.stderr)
        record = json.loads(process.stdout)
        linux.contract.validate_result_record(record)
        self.assertEqual(record["result"], "ERROR")
        self.assertEqual(record["cleanup_result"]["status"], "incomplete")
        self.assertEqual(record["platform_facts"]["execution_method"], "guest-result-unavailable")


if __name__ == "__main__":
    unittest.main()
