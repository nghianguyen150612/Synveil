"""Checks that the P045 upload sources belong to the intended Xcode targets."""
import re
import unittest
from pathlib import Path

IOS = Path(__file__).resolve().parents[2]


class FileUploadRegistrationTests(unittest.TestCase):
    def test_production_and_test_sources_are_registered_once(self):
        project = (IOS / "Synveil.xcodeproj/project.pbxproj").read_text()
        objects = re.findall(r"^\s*([A-F0-9]{24}) /\*.*?\*/ =", project, re.M)
        self.assertEqual(len(objects), len(set(objects)))
        source_phases = re.findall(
            r"isa = PBXSourcesBuildPhase;.*?files = \((.*?)\);", project, re.S
        )
        groups = re.findall(r"isa = PBXGroup;.*?children = \((.*?)\);", project, re.S)
        self.assertEqual(len(source_phases), 3)

        sources = [
            "Application/Transfer/AuthenticatedUploadRepository.swift",
            "Application/Transfer/FileUploadCoordinator.swift",
            "Domain/Transfer/UploadModels.swift",
            "Domain/Transfer/UploadSessionDTO.swift",
            "Features/Transfer/FileUploadControls.swift",
            "Features/Transfer/FileUploadViewModel.swift",
            "Infrastructure/Network/URLSessionRawUploadTransport.swift",
            "Infrastructure/Persistence/FileUploadSQLiteSchema.swift",
            "Infrastructure/Transfer/UploadStagingStore.swift",
            "Tests/SynveilTests/FileUploadTests.swift",
            "Tests/SynveilTests/FileUploadStorageTests.swift",
            "Tests/SynveilTests/FileUploadViewTests.swift",
            "Tests/SynveilTests/FileUploadCoordinatorTests.swift",
        ]
        for path in sources:
            self.assertTrue((IOS / path).is_file(), path)
            reference = re.search(
                r"([A-F0-9]{24}) /\*.*?\*/ = \{isa = PBXFileReference;[^\n]*path = "
                + re.escape(path)
                + r";",
                project,
            )
            self.assertIsNotNone(reference, path)
            reference_id = reference.group(1)
            self.assertEqual(sum(reference_id in group for group in groups), 1, path)
            build = re.search(
                r"([A-F0-9]{24}) /\*.*?\*/ = \{isa = PBXBuildFile; fileRef = "
                + reference_id
                + r"\b",
                project,
            )
            self.assertIsNotNone(build, path)
            target = 1 if path.startswith("Tests/") else 0
            for phase_index, source_phase in enumerate(source_phases):
                if phase_index == target:
                    self.assertIn(build.group(1), source_phase, path)
                else:
                    self.assertNotIn(build.group(1), source_phase, path)

    def test_picker_is_single_file_and_controls_are_explicit(self):
        controls = (IOS / "Features/Transfer/FileUploadControls.swift").read_text()
        self.assertIn(".fileImporter(", controls)
        self.assertIn("allowsMultipleSelection: false", controls)
        self.assertIn('Button("Pause"', controls)
        self.assertIn('Button("Abort Upload"', controls)
        self.assertIn('Button("Check Server Status"', controls)
        self.assertIn('Label("Upload File"', controls)
        self.assertIn('Label("Replace File Content"', controls)


if __name__ == "__main__":
    unittest.main()
