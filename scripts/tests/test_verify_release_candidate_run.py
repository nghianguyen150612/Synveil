from __future__ import annotations

import hashlib
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
import release_manifest

SPEC = importlib.util.spec_from_file_location(
    "verify_release_candidate_run", ROOT / "scripts/verify_release_candidate_run.py"
)
verify_module = importlib.util.module_from_spec(SPEC)
assert SPEC and SPEC.loader
SPEC.loader.exec_module(verify_module)
SOURCE = "a" * 40
REPOSITORY = "nghianguyen150612/Synveil"


class VerifyReleaseCandidateRunTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self._write_candidate()

    def tearDown(self):
        self.temporary.cleanup()

    def _write_candidate(self):
        rows = [
            ("appimage-linux-x86_64", "appimage", "Synveil-0.2.0-x86_64.AppImage"),
            ("debian-x86_64-deb", "deb", "synveil_0.2.0_amd64.deb"),
            ("fedora-x86_64-rpm", "rpm", "synveil-0.2.0-1.x86_64.rpm"),
            ("windows-x86_64-installer", "windows_installer", "SynveilSetup.exe"),
            ("windows-x86_64-portable", "windows_portable_zip", "synveil-0.2.0-windows-x86_64.zip"),
        ]
        artifacts = []
        for artifact_id, kind, filename in rows:
            (self.root / filename).write_bytes((filename + " exact test bytes").encode())
            item = {
                "id": artifact_id,
                "artifact_type": kind,
                "filename": filename,
                "platform": "windows" if kind.startswith("windows_") else "linux",
                "architecture": "x86_64",
                "role": {
                    "appimage": "portable",
                    "deb": "native_package",
                    "rpm": "native_package",
                    "windows_installer": "primary_installer",
                    "windows_portable_zip": "portable",
                }[kind],
                "components": ["synveil-desktop", "synveil-client"],
            }
            if kind in {"deb", "rpm"}:
                item["package_metadata"] = {
                    "format": kind,
                    "package_name": "synveil",
                    "package_version": "0.2.0",
                    "package_architecture": "amd64" if kind == "deb" else "x86_64",
                    **({"package_release": "1"} if kind == "rpm" else {}),
                }
            artifacts.append(release_manifest.artifact_from_spec(self.root, "0.2.0", json.dumps(item)))
        artifacts.sort(key=lambda item: item["id"])
        manifest = {
            "schema_version": 1,
            "product": "Synveil",
            "product_version": "0.2.0",
            "source_commit": SOURCE,
            "artifacts": artifacts,
        }
        manifest_path = self.root / "SYNVEIL-RELEASE-MANIFEST.json"
        release_manifest.write(manifest_path, manifest)
        evidence = {
            "schema_version": 1,
            "status": "VERIFIED_UNSIGNED_CANDIDATE",
            "product_version": "0.2.0",
            "source_commit": SOURCE,
            "artifacts": [
                {key: item[key] for key in ("artifact_type", "filename", "size_bytes", "sha256")}
                for item in artifacts
            ],
        }
        (self.root / "candidate-evidence.json").write_text(json.dumps(evidence), encoding="utf-8")

    def test_accepts_only_successful_exact_release_candidate_run(self):
        run = {
            "id": 12345,
            "path": ".github/workflows/release-artifact-candidate.yml@refs/heads/release/test",
            "event": "workflow_dispatch",
            "status": "completed",
            "conclusion": "success",
            "head_sha": SOURCE,
            "head_repository": {"full_name": REPOSITORY},
        }
        self.assertEqual(
            verify_module.validate_run_record(run, "12345", REPOSITORY)["source_commit"],
            SOURCE,
        )

    def test_rejects_pull_request_run_and_other_repositories(self):
        base = {
            "id": 12345,
            "path": ".github/workflows/release-artifact-candidate.yml@refs/heads/release/test",
            "event": "pull_request",
            "status": "completed",
            "conclusion": "success",
            "head_sha": SOURCE,
            "head_repository": {"full_name": REPOSITORY},
        }
        with self.assertRaises(verify_module.CandidateError):
            verify_module.validate_run_record(base, "12345", REPOSITORY)
        base["event"] = "workflow_dispatch"
        base["head_repository"] = {"full_name": "fork/Synveil"}
        with self.assertRaises(verify_module.CandidateError):
            verify_module.validate_run_record(base, "12345", REPOSITORY)

    def test_binds_all_five_product_files_to_run_source_and_digest(self):
        result = verify_module.validate_candidate_files(self.root, SOURCE)
        self.assertEqual(result["status"], "VERIFIED_UNSIGNED_CANDIDATE")
        self.assertEqual(result["source_commit"], SOURCE)
        self.assertEqual(len(result["artifacts"]), 5)
        self.assertEqual(
            {item["artifact_type"] for item in result["artifacts"]},
            verify_module.EXPECTED_TYPES,
        )
        self.assertEqual(
            result["manifest_sha256"],
            hashlib.sha256((self.root / "SYNVEIL-RELEASE-MANIFEST.json").read_bytes()).hexdigest(),
        )

    def test_rejects_changed_file_and_wrong_source(self):
        selected = self.root / "SynveilSetup.exe"
        selected.write_bytes(b"substituted setup")
        with self.assertRaises(verify_module.CandidateError):
            verify_module.validate_candidate_files(self.root, SOURCE)
        self._write_candidate()
        with self.assertRaises(verify_module.CandidateError):
            verify_module.validate_candidate_files(self.root, "b" * 40)

    def test_rejects_other_artifact_name(self):
        with self.assertRaises(verify_module.CandidateError):
            verify_module.verify("12345", "windows-p028-candidate", REPOSITORY, SOURCE, self.root)

    def test_rejects_candidate_from_another_source_revision(self):
        with patch.object(
            verify_module,
            "fetch_run",
            return_value={"source_commit": "b" * 40},
        ):
            with self.assertRaisesRegex(verify_module.CandidateError, "differs from the native acceptance"):
                verify_module.verify("12345", verify_module.EXPECTED_ARTIFACT, REPOSITORY, SOURCE, self.root)


if __name__ == "__main__":
    unittest.main()
