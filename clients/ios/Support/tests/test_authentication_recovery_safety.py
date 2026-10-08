"""Presentation-boundary security regressions; behavioral lifecycle tests run in XCTest."""
import re
import unittest
from pathlib import Path


IOS = Path(__file__).resolve().parents[2]


class AuthenticationRecoverySafetyTests(unittest.TestCase):
    def test_recovery_views_cannot_authenticate_enroll_or_delete_credentials(self):
        sources = [IOS / "App/RootView.swift", IOS / "Features/Authentication/AuthenticationRecoveryView.swift"]
        for source in sources:
            text = source.read_text()
            for forbidden in [r"\.state\s*=(?!=)", r"markAuthenticated\(", r"requireEnrollment\(",
                              r"\.exchange\(", r"\.delete\(", r"\.store\("]:
                self.assertIsNone(re.search(forbidden, text), source.name)

    def test_recovery_uses_only_existing_controller_operations(self):
        text = (IOS / "App/RootView.swift").read_text()
        calls = set(re.findall(r"sessionController\.(\w+)\(", text))
        self.assertEqual(calls, {"retrySessionRestoration", "requestLogout"})

    def test_recovery_diagnostics_do_not_dump_error_objects(self):
        text = (IOS / "Features/Authentication/AuthenticationRecoveryView.swift").read_text()
        for forbidden in ["debugDescription", "localizedDescription", "Authorization", "rawTokenInput", "record.credential"]:
            self.assertNotIn(forbidden, text)
        self.assertIn("presentation.requestID", text)
        self.assertIn("textSelection(.enabled)", text)

    def test_recovery_content_scales_and_has_stable_accessibility_elements(self):
        text = (IOS / "Features/Authentication/AuthenticationRecoveryView.swift").read_text()
        self.assertIsNone(re.search(r"\.frame\([^)]*(?:height|maxHeight):", text))
        self.assertIn("ScrollView", text)
        self.assertIn(".isHeader", text)
        for identifier in ["title", "message", "protection", "next-step", "diagnostics", "request-id", "retry-progress"]:
            self.assertIn(f"synveil.recovery.{identifier}", text)
        self.assertIn(".accessibilityLabel", text)
        self.assertIn(".accessibilityHint", text)

    def test_new_files_are_registered_in_the_correct_xcode_targets(self):
        project = (IOS / "Synveil.xcodeproj/project.pbxproj").read_text()
        phases = re.findall(r"isa = PBXSourcesBuildPhase;.*?files = \((.*?)\);", project, re.S)
        self.assertEqual(len(phases), 2)
        for name in ["AuthenticationRecoveryPresentation.swift", "AuthenticationRecoveryView.swift"]:
            self.assertIn(f"{name} in Sources", phases[0])
            self.assertNotIn(f"{name} in Sources", phases[1])
        self.assertIn("AuthenticationRecoveryTests.swift in Sources", phases[1])
        self.assertNotIn("AuthenticationRecoveryTests.swift in Sources", phases[0])


if __name__ == "__main__":
    unittest.main()
