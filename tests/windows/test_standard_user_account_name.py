#!/usr/bin/env python3
"""Keep disposable local acceptance accounts within the Windows SAM limit."""

from __future__ import annotations

import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
RUNNER = ROOT / "scripts/invoke-windows-standard-user-test.ps1"
PER_USER_TEST = ROOT / "scripts/test-windows-per-user-installation.ps1"


class StandardUserAccountNameTests(unittest.TestCase):
    def test_generated_name_fits_windows_local_account_limit(self) -> None:
        source = RUNNER.read_text(encoding="utf-8")
        match = re.search(
            r"(?m)^\$user\s*=\s*'([^']+)'\s*\+\s*\[Guid\]::NewGuid\(\)"
            r"\.ToString\('N'\)\.Substring\(0,(\d+)\)\s*$",
            source,
        )
        self.assertIsNotNone(match, "disposable account name must use the reviewed generated form")
        assert match is not None
        prefix, suffix_length_text = match.groups()
        suffix_length = int(suffix_length_text)
        self.assertRegex(prefix, r"^[A-Za-z0-9_]+$")
        self.assertGreater(suffix_length, 0)
        self.assertLessEqual(
            len(prefix) + suffix_length,
            20,
            "Windows local SAM account names cannot exceed 20 characters",
        )

    def test_desktop_known_folder_has_a_profile_fallback(self) -> None:
        source = PER_USER_TEST.read_text(encoding="utf-8")
        self.assertIn(
            "$desktopFolder = [Environment]::GetFolderPath([Environment+SpecialFolder]::Desktop)",
            source,
        )
        self.assertIn("if ([string]::IsNullOrWhiteSpace($desktopFolder))", source)
        self.assertIn("$desktopFolder = Join-Path $profileRoot 'Desktop'", source)
        self.assertIn("New-Item -Path $desktopFolder -ItemType Directory -Force", source)
        self.assertIn("$desktop = Join-Path $desktopFolder 'Synveil.lnk'", source)
        self.assertNotIn("Join-Path ([Environment]::GetFolderPath('Desktop'))", source)


if __name__ == "__main__":
    unittest.main()
