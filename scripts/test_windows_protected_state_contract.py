#!/usr/bin/env python3
"""Regression checks for protected-state policy wiring (not ACL qualification)."""
import re
import unittest
from pathlib import Path

SOURCE = Path(__file__).with_name('protect-windows-native-run-state.ps1').read_text(encoding='utf-8')


class ProtectedStateContractTests(unittest.TestCase):
    def test_admin_only_directories_are_verified_with_same_policy(self):
        for name in ('observerInbox', 'evidenceRoot'):
            self.assertEqual(1, len(re.findall(rf'(?m)^\s*Set-RunAcl \${name} \$false \$false\s*$', SOURCE)))
            self.assertEqual(2, len(re.findall(rf'(?m)^\s*Get-ExpectedAcl \${name} \$false \$false\s*$', SOURCE)))

    def test_account_sid_includes_rid(self):
        match = re.search(r"ValidatePattern\('([^']+)'\)\]\[string\]\$SubjectSid", SOURCE)
        self.assertIsNotNone(match)
        pattern = re.compile(match.group(1))
        self.assertIsNotNone(pattern.fullmatch('S-1-5-21-123-456-789-1001'))
        self.assertIsNone(pattern.fullmatch('S-1-5-21-123-456-789'))

    def test_privileged_identity_is_revalidated(self):
        self.assertIn("throw 'P028_SUBJECT_IDENTITY_MISMATCH'", SOURCE)
        self.assertIn("throw 'P028_CONTROLLER_IDENTITY_MISMATCH'", SOURCE)

    def test_unverified_native_advancement_is_blocked(self):
        self.assertIn("throw 'P028_NATIVE_AUTHORITY_UNAVAILABLE'", SOURCE)
        self.assertLess(SOURCE.index("throw 'P028_NATIVE_AUTHORITY_UNAVAILABLE'"),
                        SOURCE.index('$raw = [P028SafeEvidenceReader]::Read'))


if __name__ == '__main__':
    unittest.main()
