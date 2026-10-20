from __future__ import annotations

import importlib.util
import os
from pathlib import Path
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
spec = importlib.util.spec_from_file_location("sign_p036_inventory", ROOT / "scripts/sign_p036_inventory.py")
signer = importlib.util.module_from_spec(spec)
assert spec and spec.loader
spec.loader.exec_module(signer)


class P036SigningTests(unittest.TestCase):
    def test_missing_inventory_is_an_external_input_blocker(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            keyring = root / "public.gpg"
            keyring.write_bytes(b"public only")
            names = (signer.FINGERPRINT_ENV, "SYNVEIL_P036_TRUSTED_KEYRING")
            saved = {name: os.environ.get(name) for name in names}
            os.environ[signer.FINGERPRINT_ENV] = "A" * 40
            os.environ["SYNVEIL_P036_TRUSTED_KEYRING"] = str(keyring)
            signature = root / "out" / "production-artifacts.json.sig"
            try:
                with self.assertRaisesRegex(signer.SigningError, "BLOCKED_BY_EXTERNAL_INPUT"):
                    signer.sign_inventory(root / "missing.json", signature, root / "artifacts")
            finally:
                for name, value in saved.items():
                    if value is None:
                        os.environ.pop(name, None)
                    else:
                        os.environ[name] = value
            self.assertFalse(signature.exists())


if __name__ == "__main__":
    unittest.main()
