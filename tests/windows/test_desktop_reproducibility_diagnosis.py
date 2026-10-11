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
            self.assertIn("PE header and section details are in windows-details.txt", binaries)
            self.assertIn("first differing byte offset: 3", binaries)
            self.assertNotIn("readelf: Error", result.stderr)

    @staticmethod
    def _minimal_pe(timestamp: int, section_byte: int) -> bytes:
        pe_offset = 0x80
        optional_size = 0xF0
        section_table = pe_offset + 4 + 20 + optional_size
        raw_offset = 0x200
        image = bytearray(raw_offset + 0x20)
        image[:2] = b"MZ"
        image[0x3C:0x40] = pe_offset.to_bytes(4, "little")
        image[pe_offset : pe_offset + 4] = b"PE\0\0"
        coff = pe_offset + 4
        image[coff : coff + 2] = (0x8664).to_bytes(2, "little")
        image[coff + 2 : coff + 4] = (1).to_bytes(2, "little")
        image[coff + 4 : coff + 8] = timestamp.to_bytes(4, "little")
        image[coff + 16 : coff + 18] = optional_size.to_bytes(2, "little")
        image[coff + 18 : coff + 20] = (0x22).to_bytes(2, "little")
        optional = coff + 20
        image[optional : optional + 2] = (0x20B).to_bytes(2, "little")
        image[optional + 24 : optional + 32] = (0x140000000).to_bytes(8, "little")
        image[optional + 32 : optional + 36] = (0x1000).to_bytes(4, "little")
        image[optional + 36 : optional + 40] = (0x200).to_bytes(4, "little")
        image[optional + 56 : optional + 60] = (0x2000).to_bytes(4, "little")
        image[optional + 60 : optional + 64] = raw_offset.to_bytes(4, "little")
        image[optional + 108 : optional + 112] = (16).to_bytes(4, "little")
        section = section_table
        image[section : section + 8] = b".text\0\0\0"
        image[section + 8 : section + 12] = (0x20).to_bytes(4, "little")
        image[section + 12 : section + 16] = (0x1000).to_bytes(4, "little")
        image[section + 16 : section + 20] = (0x20).to_bytes(4, "little")
        image[section + 20 : section + 24] = raw_offset.to_bytes(4, "little")
        image[section + 36 : section + 40] = (0x60000020).to_bytes(4, "little")
        image[raw_offset] = section_byte
        return bytes(image)

    def test_valid_pe_report_identifies_header_section_and_object_differences(self) -> None:
        target_root = ROOT / "target"
        target_root.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(prefix="synveil-valid-pe-diagnosis-", dir=target_root) as directory:
            root = Path(directory)
            build_a, build_b = root / "build-a", root / "build-b"
            report = root / "windows-details.txt"
            for build in (build_a, build_b):
                (build / "release/deps").mkdir(parents=True)
                (build / "release/synveil-client.exe").write_bytes(self._minimal_pe(0, 0x11))
                (build / "release/synveil-desktop.exe").write_bytes(self._minimal_pe(0, 0x22))
                (build / "release/deps/client.obj").write_bytes(b"object-a")
            (build_b / "release/synveil-desktop.exe").write_bytes(self._minimal_pe(123, 0x33))
            (build_b / "release/deps/client.obj").write_bytes(b"object-b")

            result = subprocess.run(
                [
                    "python",
                    str(ROOT / "scripts/diagnose-windows-reproducibility.py"),
                    str(build_a),
                    str(build_b),
                    str(report),
                ],
                cwd=ROOT,
                text=True,
                capture_output=True,
                check=False,
            )

            self.assertEqual(result.returncode, 0, f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}")
            details = report.read_text(encoding="utf-8")
            self.assertIn("coff.timestamp: 0 -> 123", details)
            self.assertIn(".text:", details)
            self.assertIn("release/deps/client.obj", details)
            self.assertIn("first-difference=7", details)


if __name__ == "__main__":
    unittest.main(verbosity=2)
