#!/usr/bin/env python3
"""Regression tests for Windows release-binary mismatch diagnostics."""

from __future__ import annotations

import os
import subprocess
import tempfile
import unittest
from pathlib import Path, PureWindowsPath
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
DIAGNOSIS = ROOT / "scripts/diagnose-desktop-reproducibility.sh"


def bash_executable(
    platform_name: str = os.name,
    environment: dict[str, str] | None = None,
) -> str:
    if platform_name != "nt":
        return "bash"

    env = os.environ if environment is None else environment
    configured = env.get("SYNVEIL_GIT_BASH")
    if configured:
        candidate = str(PureWindowsPath(configured))
    else:
        program_files = env.get("ProgramFiles")
        if not program_files:
            raise RuntimeError("ProgramFiles is unavailable; refusing PATH-based bash selection")
        candidate = str(PureWindowsPath(program_files) / "Git" / "bin" / "bash.exe")

    if not os.path.isfile(candidate):
        raise FileNotFoundError(f"Git for Windows bash is unavailable at the selected path: {candidate}")
    return candidate


class DesktopReproducibilityDiagnosisTests(unittest.TestCase):
    def test_windows_bash_resolution_ignores_path_order(self) -> None:
        environment = {
            "ProgramFiles": r"C:\Program Files",
            "PATH": r"C:\Windows\System32;C:\Program Files\Git\bin",
        }
        expected = r"C:\Program Files\Git\bin\bash.exe"
        with patch("os.path.isfile", return_value=True) as is_file:
            selected = bash_executable("nt", environment)

        self.assertEqual(selected, expected)
        is_file.assert_called_once_with(expected)

    def test_pe_executable_mismatches_are_reported_without_elf_tools(self) -> None:
        target_root = ROOT / "target"
        target_root.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(prefix="synveil-pe-diagnosis-", dir=target_root) as directory:
            root = Path(directory)
            relative_root = root.relative_to(ROOT).as_posix()
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
                [
                    bash_executable(),
                    DIAGNOSIS.relative_to(ROOT).as_posix(),
                    f"{relative_root}/build-a",
                    f"{relative_root}/build-b",
                    f"{relative_root}/report",
                ],
                cwd=ROOT,
                text=True,
                capture_output=True,
                check=False,
            )

            self.assertEqual(
                result.returncode,
                0,
                f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}",
            )
            binaries = (report / "binaries.txt").read_text(encoding="utf-8")
            self.assertIn("synveil-client: DIFFERS", binaries)
            self.assertIn("synveil-desktop: DIFFERS", binaries)
            self.assertIn("PE executable; ELF section analysis skipped", binaries)
            self.assertIn("first differing byte offset: 3", binaries)
            self.assertNotIn("readelf: Error", result.stderr)


if __name__ == "__main__":
    unittest.main(verbosity=2)
