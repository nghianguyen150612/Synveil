import importlib.util
from pathlib import Path
import tempfile
import unittest
from urllib.request import Request

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "appimage_runtime_acquisition", ROOT / "scripts/acquire-appimage-runtime.py"
)
runtime = importlib.util.module_from_spec(SPEC)
assert SPEC.loader
SPEC.loader.exec_module(runtime)


class AppImageRuntimeAcquisitionTests(unittest.TestCase):
    def test_upstream_release_pin_is_versioned(self):
        self.assertEqual("20251108", runtime.RELEASE_TAG)
        self.assertEqual("dd6cebedcbddde9c82f89b011e8e1d40b6e43868", runtime.SOURCE_REVISION)
        self.assertNotIn("latest", runtime.RELEASE_API)
        self.assertNotIn("continuous", runtime.RELEASE_API)
        self.assertEqual("sha256:2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d",
                         f"sha256:{runtime.ASSETS['runtime-x86_64']['sha256']}")

    def test_asset_bytes_require_exact_size_and_publisher_digest(self):
        data = b"authenticated runtime fixture"
        import hashlib
        digest = hashlib.sha256(data).hexdigest()
        runtime._check_bytes("fixture", data, len(data), digest)
        with self.assertRaises(runtime.AcquisitionError):
            runtime._check_bytes("fixture", data, len(data) + 1, digest)
        with self.assertRaises(runtime.AcquisitionError):
            runtime._check_bytes("fixture", data, len(data), "0" * 64)

    def test_redirect_policy_rejects_unapproved_hosts(self):
        handler = runtime._RestrictedRedirect(runtime.RELEASE_ASSET_HOSTS)
        with self.assertRaises(runtime.AcquisitionError):
            handler.redirect_request(Request("https://github.com/AppImage/type2-runtime"), None,
                                     302, "Found", {}, "https://attacker.example/runtime")

    def test_runtime_architecture_is_checked_from_elf_header(self):
        header = bytearray(20)
        header[:6] = b"\x7fELF\x02\x01"
        header[16:18] = (3).to_bytes(2, "little")
        header[18:20] = (62).to_bytes(2, "little")
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "runtime"
            path.write_bytes(header)
            runtime._verify_elf_x86_64(path)
            header[18:20] = (183).to_bytes(2, "little")
            path.write_bytes(header)
            with self.assertRaises(runtime.AcquisitionError):
                runtime._verify_elf_x86_64(path)


if __name__ == "__main__":
    unittest.main()
