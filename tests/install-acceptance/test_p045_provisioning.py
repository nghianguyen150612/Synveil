"""Provisioning shell fixtures; never native graphical acceptance evidence."""
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
                commands = [line for line in data.splitlines() if line.startswith("  - [ bash, -lc, '")]
                self.assertEqual(len(commands), 1, "no independent command may mark a failed provisioner ready")
                program = commands[0].removeprefix("  - [ bash, -lc, '").removesuffix("' ]")
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
                completed = subprocess.run(["bash", "-c", program], env=env, capture_output=True, text=True, timeout=5)
                self.assertEqual(completed.returncode, 0, completed.stderr)
                self.assertTrue(marker.exists())
                config = root / ("etc/gdm3/custom.conf" if platform == "ubuntu" else "etc/gdm/custom.conf")
                self.assertEqual(config.read_text().splitlines()[0], "[daemon]")
                self.assertIn("AutomaticLogin=synveil-acceptance", config.read_text().splitlines())
                self.assertNotIn(r"\n", config.read_text())
                if platform == "fedora":
                    self.assertIn("environment install workstation-product-environment", program)


if __name__ == "__main__":
    unittest.main()
