"""ELF metadata format fixtures; never product or native acceptance evidence."""
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class AbsentBuildIdTests(unittest.TestCase):
    def setUp(self):
        (ROOT / "target").mkdir(exist_ok=True)
        self.tmp = tempfile.TemporaryDirectory(prefix="p045-elf-format-fixture-", dir=ROOT / "target")
        self.addCleanup(self.tmp.cleanup)
        self.directory = Path(self.tmp.name)
        self.source = self.directory / "fixture.c"
        self.source.write_text("int main(void) { return 0; }\n")
        self.manifest = self.directory / "fixture-manifest.txt"
        self.artifacts = []
        for index in range(3):
            artifact = self.directory / f"elf-format-fixture-{index}"
            subprocess.run(["cc", "-s", "-Wl,--build-id=none", str(self.source), "-o", str(artifact)], check=True, capture_output=True, timeout=30)
            self.artifacts.append(artifact)

    def write_manifest(self):
        pairs = [f"{p.relative_to(ROOT)}:{p}" for p in self.artifacts]
        subprocess.run(["bash", "-euo", "pipefail", "-c",
                        'source "$1"; synveil_write_linux_artifact_manifest "$2" "$3" 1700000000 "${@:4}"',
                        "elf-format-fixture", str(ROOT / "deploy/packages/common/reproducible.sh"),
                        str(self.manifest), str(ROOT), *pairs], check=True, capture_output=True, timeout=60)

    def validate(self):
        return subprocess.run(["bash", str(ROOT / "scripts/validate-release-artifacts.sh"),
                               f"--manifest={self.manifest}"], capture_output=True, text=True, timeout=60)

    def test_absent_build_id_round_trips_with_exact_bytes(self):
        self.write_manifest()
        entries = self.manifest.read_text().split("artifacts=sha256 size build_id path\n", 1)[1].splitlines()
        self.assertEqual(len(entries), 3)
        self.assertTrue(all(len(e.split()) == 4 and e.split()[2] == "none" for e in entries))
        result = self.validate()
        self.assertEqual(result.returncode, 0, result.stderr)
        data = bytearray(self.artifacts[0].read_bytes())
        data[-1] ^= 1
        self.artifacts[0].write_bytes(data)
        self.assertNotEqual(self.validate().returncode, 0)

    def test_present_gnu_build_id_remains_forbidden(self):
        subprocess.run(["cc", "-s", "-Wl,--build-id=sha1", str(self.source), "-o", str(self.artifacts[0])], check=True, capture_output=True, timeout=30)
        self.write_manifest()
        result = self.validate()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("ELF build ID is present", result.stderr)

    def test_false_build_id_marker_is_rejected(self):
        self.write_manifest()
        self.manifest.write_text(self.manifest.read_text().replace(" none ", " deadbeef ", 1))
        result = self.validate()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("release artifact manifest mismatch", result.stderr)


if __name__ == "__main__":
    unittest.main()
