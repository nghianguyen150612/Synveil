from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[2]


class HostServerInputContractTests(unittest.TestCase):
    def test_server_input_uses_server_notice_not_desktop_package_notice(self):
        builder = (ROOT / "scripts/build-host-server-input.sh").read_text(encoding="utf-8")
        notice = (ROOT / "deploy/server/NOTICE").read_text(encoding="utf-8").lower()
        self.assertIn('"$host_input_repo/deploy/server/NOTICE"', builder)
        self.assertNotIn('"$host_input_repo/deploy/NOTICE"', builder)
        self.assertIn("server api", notice)
        self.assertIn("background worker", notice)
        self.assertIn("migration utility", notice)
        self.assertIn("host helper", notice)
        self.assertIn("does not enumerate third-party rust dependency licenses", notice)
        self.assertNotIn("desktop shell", notice)


if __name__ == "__main__":
    unittest.main()
