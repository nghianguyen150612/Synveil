from __future__ import annotations

import importlib.util
import os
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
import sys
sys.path.insert(0, str(ROOT / "scripts"))
spec = importlib.util.spec_from_file_location("sign_rpm_candidate", ROOT / "scripts/sign_rpm_candidate.py")
signer = importlib.util.module_from_spec(spec)
assert spec and spec.loader
spec.loader.exec_module(signer)


class RpmSigningTests(unittest.TestCase):
    def test_missing_approved_signer_blocks_before_candidate_mutation(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            rpm = root / "synveil-0.2.0-1.x86_64.rpm"
            rpm.write_bytes(b"unsigned candidate bytes")
            before = rpm.read_bytes()
            names = (signer.PRIVATE_KEY_ENV, signer.PUBLIC_KEY_ENV, signer.FINGERPRINT_ENV)
            saved = {name: os.environ.pop(name, None) for name in names}
            try:
                with self.assertRaisesRegex(signer.SigningError, "BLOCKED_SIGNING_INPUT"):
                    signer.sign(root / "manifest.json", root)
            finally:
                for name, value in saved.items():
                    if value is not None:
                        os.environ[name] = value
            self.assertEqual(rpm.read_bytes(), before)
            self.assertFalse((root / "rpm-signing-evidence.json").exists())


if __name__ == "__main__":
    unittest.main()
