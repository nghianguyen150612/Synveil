#!/usr/bin/env python3
"""Regression tests for Windows release-binary mismatch diagnostics."""

from __future__ import annotations

import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DIAGNOSIS = ROOT / "scripts/diagnose-desktop-reproducibility.sh"


class DesktopReproducibilityDiagnosisTests(unittest.TestCase):
    def test_pe_executable_mismatches_are_reported_without_elf_tools(self) -> None:
        with tempfile.TemporaryDirectory(prefix="synveil-pe-diagnosis-") as directory:
            root = Path(directory)
            build_a = root / "build-a"
            build_b = root / "build-b"
            report = root / "report"
            for build in (build_a, build_b):
                (build / "release").mkdir(parents=True)

            (build_a / "release/synveil-client.exe").write_bytes(b"MZ" + b"A" * 254)
            (build_b / "release/synveil-client.exe").write_bytes(b"MZ" + b"B" * 254)
            (build_a / "release/synveil-desktop.exe").write_bytes(b"MZ" + b"C" * 254)
            (build_b / "release/synveil-desktop.exe").write_bytes(b"MZ" + b"D" * 254)

            result = subprocess.run(
                ["bash", str(DIAGNOSIS), str(build_a), str(build_b), str(report)],
                text=True,
                capture_output=True,
                check=False,
            )

            self.assertEqual(result.returncode, 0, result.stderr)
            binaries = (report / "binaries.txt").read_text(encoding="utf-8")
            self.assertIn("synveil-client: DIFFERS", binaries)
            self.assertIn("synveil-desktop: DIFFERS", binaries)
            self.assertIn("PE executable; ELF section analysis skipped", binaries)
            self.assertIn("first differing byte offset: 3", binaries)
            self.assertNotIn("readelf: Error", result.stderr)


if __name__ == "__main__":
    unittest.main(verbosity=2)
