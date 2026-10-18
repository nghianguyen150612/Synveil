"""Verify the complete download source inventory reaches the correct native target."""

import re
import unittest
from pathlib import Path

IOS = Path(__file__).resolve().parents[2]


class FileDownloadRegistrationTests(unittest.TestCase):
    def test_all_download_sources_have_exact_native_target_membership(self):
        project = (IOS / "Synveil.xcodeproj/project.pbxproj").read_text()
        phases = re.findall(r"isa = PBXSourcesBuildPhase;.*?files = \((.*?)\);", project, re.S)
        groups = re.findall(r"isa = PBXGroup;.*?children = \((.*?)\);", project, re.S)
        files = []
        for folder in ["Domain/Transfer", "Application/Transfer", "Features/Transfer", "Infrastructure/Transfer"]:
            files.extend((IOS / folder).glob("*.swift"))
        files.extend([
            IOS / "Infrastructure/Network/URLSessionFileContentTransport.swift",
            IOS / "Infrastructure/Persistence/FileDownloadSQLiteSchema.swift",
        ])
        files.extend((IOS / "Tests/SynveilTests").glob("FileDownload*.swift"))
        self.assertGreaterEqual(len(files), 14)
        for file in files:
            path = file.relative_to(IOS).as_posix()
            with self.subTest(path=path):
                refs = re.findall(r"([A-F0-9]{24}) /\*.*?\*/ = \{isa = PBXFileReference;[^\n]*path = " + re.escape(path) + r";", project)
                self.assertEqual(len(refs), 1)
                builds = re.findall(r"([A-F0-9]{24}) /\*.*?\*/ = \{isa = PBXBuildFile; fileRef = " + refs[0], project)
                self.assertEqual(len(builds), 1)
                target = 1 if path.startswith("Tests/") else 0
                self.assertEqual(phases[target].count(builds[0]), 1)
                self.assertNotIn(builds[0], phases[1 - target])
                self.assertEqual(sum(group.count(refs[0]) for group in groups), 1)


if __name__ == "__main__":
    unittest.main()
