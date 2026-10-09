"""Provisioning shell fixtures; never native graphical acceptance evidence."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class ProvisioningBoundaryTests(unittest.TestCase):
    def test_marker_requires_success_and_gdm_configuration_has_real_lines(self):
        source = (ROOT / "scripts/linux-acceptance-vm.sh").read_text()
        function = source[source.index("write_cloud_init() {"):source.index("write_vm_metadata() {")]
        for platform in ("ubuntu", "fedora"):
            with self.subTest(platform=platform), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                seed = root / "seed"
                output = subprocess.run(["bash", "-c", function + '\nwrite_cloud_init "$1" fixture-password fixture-key "$2"', "fixture", str(seed), platform], capture_output=True, text=True, timeout=10)
                self.assertEqual(output.returncode, 0, output.stderr)
                data = (seed / "user-data").read_text()
                commands = [line for line in data.splitlines() if line.startswith("  - [ bash, -lc, ")]
                self.assertEqual(len(commands), 1, "no independent command may mark a failed provisioner ready")
                program = json.loads(commands[0].removeprefix("  - ").replace("[ bash, -lc, ", '["bash", "-lc", '))[2]
                for path in (("/etc/gdm3" if platform == "ubuntu" else "/etc/gdm"), "/var/lib/synveil-acceptance"):
                    program = program.replace(path, str(root / path.lstrip("/")))
                for command in ("apt-get", "dnf", "systemctl", "xdotool"):
                    tool = root / command
                    tool.write_text('#!/bin/sh\ncase "$0" in *apt-get|*dnf) exit "${PROVISION_EXIT:-0}";; esac\nexit 0\n')
                    tool.chmod(0o755)
                env = {**os.environ, "PATH": str(root) + ":" + os.environ["PATH"]}
                failed = subprocess.run(["bash", "-c", program], env={**env, "PROVISION_EXIT": "1"}, capture_output=True, text=True, timeout=5)
                self.assertNotEqual(failed.returncode, 0)
                marker = root / "var/lib/synveil-acceptance/desktop-ready"
                self.assertFalse(marker.exists())
                self.assertEqual((root / "var/lib/synveil-acceptance/provision-exit").read_text().strip(), "1")
                completed = subprocess.run(["bash", "-c", program], env=env, capture_output=True, text=True, timeout=5)
                self.assertEqual(completed.returncode, 0, completed.stderr)
                self.assertTrue(marker.exists())
                self.assertEqual((root / "var/lib/synveil-acceptance/provision-exit").read_text().strip(), "0")
                config = root / ("etc/gdm3/custom.conf" if platform == "ubuntu" else "etc/gdm/custom.conf")
                self.assertEqual(config.read_text().splitlines()[0], "[daemon]")
                self.assertIn("AutomaticLogin=synveil-acceptance", config.read_text().splitlines())
                self.assertNotIn(r"\n", config.read_text())
                if platform == "fedora":
                    self.assertIn("dnf -y install @workstation-product-environment", program)
                    self.assertNotIn("environment install", program)

    def test_readiness_reports_provisioning_failure_before_a_desktop_probe(self):
        source = (ROOT / "scripts/linux-acceptance-vm.sh").read_text()
        function = source[source.index("wait_for_guest_readiness() {"):source.index("record_boot_diagnostics() {")]
        with tempfile.TemporaryDirectory() as tmp:
            probes = Path(tmp) / "probes"
            harness = function + r"""
BOOT_TIMEOUT_SECONDS=1
log() { :; }
guest_exec() {
    case "$3" in
      provisioning-status) printf '%s' "$PROVISION_STATUS";;
      readiness) echo desktop >> "$PROBES"; return "$READY_STATUS";;
    esac
}
if wait_for_guest_readiness 2222 fixture-key; then echo ready; else echo "$GUEST_READINESS_FAILURE"; fi
"""
            env = {**os.environ, "PROBES": str(probes), "PROVISION_STATUS": "42", "READY_STATUS": "0"}
            failed = subprocess.run(["bash", "-c", harness], env=env, capture_output=True, text=True, timeout=3)
            self.assertEqual(failed.stdout.strip(), "provisioning-failed-exit-42")
            self.assertFalse(probes.exists(), "a failed provisioner cannot probe its way to ready")
            ready = subprocess.run(["bash", "-c", harness], env={**env, "PROVISION_STATUS": "0"}, capture_output=True, text=True, timeout=3)
            self.assertEqual(ready.stdout.strip(), "ready")
            self.assertTrue(probes.exists())


if __name__ == "__main__":
    unittest.main()
