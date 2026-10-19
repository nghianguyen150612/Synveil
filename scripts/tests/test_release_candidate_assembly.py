from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
spec = importlib.util.spec_from_file_location("assemble_release_candidate", ROOT / "scripts/assemble_release_candidate.py")
assembler = importlib.util.module_from_spec(spec)
assert spec and spec.loader
spec.loader.exec_module(assembler)
VERSION = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
COMMIT = subprocess.check_output(["git", "-C", str(ROOT), "rev-parse", "HEAD"], text=True).strip()


class CandidateAssemblyTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.inbox = self.root / "inbox"
        self.inbox.mkdir()
        self.output = self.root / "candidate"
        self._write_fixture()

    def tearDown(self):
        self.temp.cleanup()

    def _write_fixture(self, source_commit=COMMIT):
        rows = [
            ("linux", "deb", "synveil_0.2.0_amd64.deb", "debian-x86_64-deb"),
            ("linux", "rpm", "synveil-0.2.0-1.x86_64.rpm", "rpm-x86_64-rpm"),
            ("appimage", "appimage", "Synveil-0.2.0-x86_64.AppImage", "appimage-linux-x86_64"),
            ("windows-zip", "windows_portable_zip", "synveil-0.2.0-windows-x86_64.zip", "windows-x86_64-portable"),
            ("windows-setup", "windows_installer", "SynveilSetup.exe", "windows-x86_64-installer"),
        ]
        grouped = {}
        for directory, kind, filename, identity in rows:
            output = self.inbox / directory
            output.mkdir(exist_ok=True)
            (output / filename).write_bytes((kind + " exact bytes").encode())
            artifact = {
                "id": identity, "artifact_type": kind, "filename": filename,
                "platform": "windows" if kind.startswith("windows_") else "linux",
                "architecture": "x86_64",
                "role": {"deb": "native_package", "rpm": "native_package", "windows_installer": "primary_installer", "windows_portable_zip": "portable", "appimage": "portable"}[kind],
                "components": ["synveil-desktop", "synveil-client"],
            }
            if kind in {"deb", "rpm"}:
                artifact["package_metadata"] = {
                    "format": kind, "package_name": "synveil", "package_version": VERSION,
                    "package_architecture": "amd64" if kind == "deb" else "x86_64",
                    **({"package_release": "1"} if kind == "rpm" else {}),
                }
            grouped.setdefault(directory, []).append(artifact)
        for directory, artifacts in grouped.items():
            output = self.inbox / directory
            args = [
                sys.executable, str(ROOT / "scripts/release_manifest.py"), "create",
                "--artifact-root", str(output), "--product-version", VERSION,
                "--source-commit", COMMIT, "--output", str(output / "SYNVEIL-RELEASE-MANIFEST.json"),
            ]
            for artifact in artifacts:
                args.extend(("--artifact", json.dumps(artifact)))
            subprocess.run(args, check=True, capture_output=True, text=True)

    def test_assembles_exact_five_artifact_matrix_and_revalidates_bytes(self):
        summary = assembler.assemble(self.inbox, self.output, COMMIT)
        manifest = json.loads((self.output / "SYNVEIL-RELEASE-MANIFEST.json").read_text())
        self.assertEqual(summary["status"], "VERIFIED_UNSIGNED_CANDIDATE")
        self.assertEqual(len(manifest["artifacts"]), 5)
        self.assertEqual({item["artifact_type"] for item in manifest["artifacts"]}, assembler.REQUIRED_TYPES)
        self.assertTrue((self.output / "Synveil-0.2.0-x86_64.AppImage").stat().st_mode & 0o111)

    def test_rejects_wrong_source_commit(self):
        with self.assertRaisesRegex(assembler.AssemblyError, "source_commit mismatch"):
            assembler.assemble(self.inbox, self.output, "f" * 40)

    def test_rejects_manifest_with_wrong_version(self):
        path = self.inbox / "linux" / "SYNVEIL-RELEASE-MANIFEST.json"
        document = json.loads(path.read_text())
        document["product_version"] = "9.9.9"
        path.write_text(json.dumps(document))
        with self.assertRaisesRegex(assembler.AssemblyError, "producer manifest failed validation"):
            assembler.assemble(self.inbox, self.output, COMMIT)

    def test_rejects_missing_producer_manifest(self):
        (self.inbox / "appimage" / "SYNVEIL-RELEASE-MANIFEST.json").unlink()
        with self.assertRaisesRegex(assembler.AssemblyError, "artifact set must contain exactly"):
            assembler.assemble(self.inbox, self.output, COMMIT)

    def test_rejects_existing_output_instead_of_overwriting(self):
        self.output.mkdir()
        marker = self.output / "keep.txt"
        marker.write_text("preserve")
        with self.assertRaisesRegex(assembler.AssemblyError, "already exists"):
            assembler.assemble(self.inbox, self.output, COMMIT)
        self.assertEqual(marker.read_text(), "preserve")


if __name__ == "__main__":
    unittest.main()
