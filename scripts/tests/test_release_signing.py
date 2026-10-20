import base64
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import tomllib
import unittest
from unittest.mock import patch
import sys

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
SPEC = importlib.util.spec_from_file_location("sign_release_metadata", ROOT / "scripts/sign_release_metadata.py")
signer = importlib.util.module_from_spec(SPEC)
assert SPEC.loader
SPEC.loader.exec_module(signer)


class ReleaseMetadataSigningTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.private = Ed25519PrivateKey.generate()
        self.seed = self.private.private_bytes(
            serialization.Encoding.Raw, serialization.PrivateFormat.Raw,
            serialization.NoEncryption(),
        )
        self.public = self.private.public_key().public_bytes(
            serialization.Encoding.Raw, serialization.PublicFormat.Raw,
        )
        self.key_id = "synveil-release-" + hashlib.sha256(self.public).hexdigest()
        self.policy = self.root / "trusted-public-policy.json"
        self.policy.write_text(json.dumps({
            "schema_version": 1,
            "purpose": "production-release-verification",
            "keys": [{"key_id": self.key_id, "scheme": "ed25519",
                      "public_key_hex": self.public.hex()}],
        }))
        self.manifest = self.root / "SYNVEIL-RELEASE-MANIFEST.json"
        self.manifest.write_text(json.dumps({
            "schema_version": 1,
            "product": "Synveil",
            "product_version": tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"],
            "source_commit": subprocess.check_output(["git", "-C", str(ROOT), "rev-parse", "HEAD"], text=True).strip(),
            "artifacts": [],
        }, sort_keys=True) + "\n")

    def tearDown(self):
        self.temp.cleanup()

    def test_protected_seed_signs_exact_manifest_and_writes_verified_descriptor(self):
        signature = self.root / signer.KINDS["manifest"]["signature"]
        descriptor = self.root / signer.KINDS["manifest"]["descriptor"]
        with patch.dict(os.environ, {signer.PRIVATE_KEY_ENV: base64.b64encode(self.seed).decode()}):
            result = signer.sign("manifest", self.manifest, signature, descriptor, self.policy)
        self.assertEqual("VERIFIED_SIGNED_CANDIDATE", result["status"])
        data = json.loads(descriptor.read_text())
        keys = signer.release_signature.load_production_keys(self.policy)
        verify = signer.release_signature.verifier(keys, {signature.name: signature.read_bytes()})
        policy = signer.release_download.ReleaseTrustPolicy(
            frozenset(), frozenset(), authentication_method="detached_signature",
            trusted_key_ids=frozenset({self.key_id}),
            allowed_signature_schemes=frozenset({"ed25519"}),
        )
        evidence = signer.release_download.authenticate_manifest(
            self.manifest.read_bytes(), policy, signature_descriptor=data, verifier=verify
        )
        self.assertEqual("AUTHENTICATED_SIGNATURE", evidence.state)

    def test_unknown_public_key_blocks_without_writing_signature(self):
        other = Ed25519PrivateKey.generate().public_key().public_bytes(
            serialization.Encoding.Raw, serialization.PublicFormat.Raw,
        )
        other_id = "synveil-release-" + hashlib.sha256(other).hexdigest()
        self.policy.write_text(json.dumps({
            "schema_version": 1,
            "purpose": "production-release-verification",
            "keys": [{"key_id": other_id, "scheme": "ed25519",
                      "public_key_hex": other.hex()}],
        }))
        with patch.dict(os.environ, {signer.PRIVATE_KEY_ENV: base64.b64encode(self.seed).decode()}):
            with self.assertRaisesRegex(signer.SigningError, "does not match"):
                signer.sign("manifest", self.manifest,
                            self.root / signer.KINDS["manifest"]["signature"],
                            self.root / signer.KINDS["manifest"]["descriptor"], self.policy)
        self.assertFalse((self.root / signer.KINDS["manifest"]["signature"]).exists())

    def test_missing_signer_is_an_explicit_blocker(self):
        with patch.dict(os.environ, {}, clear=False):
            os.environ.pop(signer.PRIVATE_KEY_ENV, None)
            with self.assertRaisesRegex(signer.SigningError, "BLOCKED_SIGNING_INPUT"):
                signer.sign("manifest", self.manifest,
                            self.root / signer.KINDS["manifest"]["signature"],
                            self.root / signer.KINDS["manifest"]["descriptor"], self.policy)


if __name__ == "__main__":
    unittest.main()
