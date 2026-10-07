"""Process-boundary tests for the VM controller; no native acceptance claims."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class VMDispatchTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        (self.root / "fixture.env").write_text("ssh_port=2222\nkey=/fixture/key\n")
        mock = self.root / "ssh"
        mock.write_text('#!/bin/sh\nprintf "%s\\n" "$@"\n')
        mock.chmod(0o755)
        (self.root / "scp").write_text('#!/bin/sh\nexit 0\n')
        (self.root / "scp").chmod(0o755)
        self.env = {**os.environ, "SYNVEIL_VM_STATE_DIR": str(self.root), "EXEC_TIMEOUT_SECONDS": "2",
                    "PATH": str(self.root) + ":" + os.environ["PATH"]}

    def run_controller(self, *args):
        return subprocess.run(["bash", str(ROOT / "scripts/linux-acceptance-vm.sh"), *args],
                              env=self.env, capture_output=True, text=True, timeout=5)

    def test_bounded_child_reloads_connection_metadata(self):
        result = self.run_controller("guest-exec", "fixture", "facts")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("2222", result.stdout)
        self.assertIn("/fixture/key", result.stdout)
        self.assertIn("cat /etc/os-release", result.stdout)

    def test_unknown_scenario_does_not_reach_ssh(self):
        result = self.run_controller("guest-exec", "fixture", "run-scenario", "INSTALL-JOURNEY-2; touch canary", "DEB", "native-clean-machine")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertIn("reviewed acceptance vocabulary", result.stderr)

    def test_guest_operation_times_out(self):
        (self.root / "ssh").write_text('#!/bin/sh\nexec sleep 10\n')
        result = self.run_controller("guest-exec", "fixture", "facts")
        self.assertEqual(result.returncode, 124)


if __name__ == "__main__":
    unittest.main()
