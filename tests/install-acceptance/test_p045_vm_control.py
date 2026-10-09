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
                                self.assertEqual(json.loads(reader.readline()), {"execute": "qmp_capabilities", "id": "capabilities"})
                                connection.sendall(b'{"return": {}, "id": "capabilities"}\r\n')
                                observed.append(json.loads(reader.readline()))
                                # Events and replies can share a read or arrive
                                # fragmented; neither is a command completion.
                                connection.sendall(b'{"event": "STOP"}\r\n{"ret')
                                connection.sendall(b'urn": "", "id": "command"}\r\n')
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
                    {"execute": "human-monitor-command", "arguments": {"command-line": "savevm clean"}, "id": "command"},
                    {"execute": "human-monitor-command", "arguments": {"command-line": "loadvm clean"}, "id": "command"},
                ])

    def test_failed_or_absent_reply_cannot_report_snapshot_saved(self):
        replies = [
            b'{"error":{"class":"GenericError","desc":"failure"},"id":"command"}\r\n',
            b'{"return":"Error: snapshot failed","id":"command"}\r\n',
            b'{"event":"STOP"}\r\n',
            b'{"return":"","id":"other"}\r\n',
            b'{"return":',
        ]
        for reply in replies:
            with self.subTest(reply=reply), tempfile.TemporaryDirectory(prefix="p045-qmp-") as directory:
                monitor = Path(directory) / "qmp.sock"
                (Path(directory) / "fixture.env").write_text(f"monitor={monitor}\n")
                with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as listener:
                    listener.bind(str(monitor))
                    listener.listen(1)
                    listener.settimeout(5)
                    failures = []

                    def serve():
                        try:
                            connection, _ = listener.accept()
                            with connection, connection.makefile("rb") as reader:
                                connection.settimeout(5)
                                connection.sendall(b'{"QMP":{}}\r\n')
                                reader.readline()
                                connection.sendall(b'{"return":{},"id":"capabilities"}\r\n')
                                reader.readline()
                                connection.sendall(reply)
                        except Exception as exc:
                            failures.append(exc)

                    worker = threading.Thread(target=serve, daemon=True)
                    worker.start()
                    result = subprocess.run(
                        ["bash", str(ROOT / "scripts/linux-acceptance-vm.sh"), "snapshot", "fixture", "clean"],
                        env={**os.environ, "SYNVEIL_VM_STATE_DIR": directory},
                        capture_output=True, text=True, timeout=10)
                    worker.join(10)
                    self.assertFalse(worker.is_alive())
                    self.assertFalse(failures, failures)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertNotIn("snapshot saved", result.stderr)


if __name__ == "__main__":
    unittest.main()
