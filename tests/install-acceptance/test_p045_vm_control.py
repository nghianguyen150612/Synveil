"""Real control-plane framing against a bounded protocol fixture, not VM evidence."""
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import threading
import unittest

ROOT = Path(__file__).resolve().parents[2]


class QmpFramingTests(unittest.TestCase):
    def test_snapshot_and_restore_send_valid_command_json(self):
        with tempfile.TemporaryDirectory(prefix="p045-qmp-") as directory:
            path = Path(directory)
            monitor = path / "qmp.sock"
            (path / "fixture.env").write_text(f"monitor={monitor}\n")
            observed = []
            failures = []
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as listener:
                listener.bind(str(monitor))
                listener.listen(2)
                listener.settimeout(5)

                def serve():
                    try:
                        for _ in range(2):
                            connection, _ = listener.accept()
                            with connection:
                                connection.settimeout(5)
                                connection.sendall(b'{"QMP": {}}\r\n')
                                reader = connection.makefile("rb")
                                self.assertEqual(json.loads(reader.readline()), {"execute": "qmp_capabilities"})
                                connection.sendall(b'{"return": {}}\r\n')
                                observed.append(json.loads(reader.readline()))
                                connection.sendall(b'{"return": ""}\r\n')
                    except Exception as exc:
                        failures.append(exc)

                worker = threading.Thread(target=serve, daemon=True)
                worker.start()
                results = []
                for command in ("snapshot", "restore"):
                    results.append(subprocess.run(
                        ["bash", str(ROOT / "scripts/linux-acceptance-vm.sh"), command, "fixture", "clean"],
                        env={**os.environ, "SYNVEIL_VM_STATE_DIR": directory},
                        capture_output=True, text=True, timeout=10))
                worker.join(10)
                self.assertFalse(worker.is_alive())
                self.assertFalse(failures, failures)
                for result in results:
                    self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(observed, [
                    {"execute": "human-monitor-command", "arguments": {"command": "savevm clean"}},
                    {"execute": "human-monitor-command", "arguments": {"command": "loadvm clean"}},
                ])


if __name__ == "__main__":
    unittest.main()
