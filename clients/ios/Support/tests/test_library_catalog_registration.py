"""Checks production composition and Xcode registration for the catalog boundary."""
import re
import unittest
from pathlib import Path

IOS = Path(__file__).resolve().parents[2]


class LibraryCatalogRegistrationTests(unittest.TestCase):
    def test_catalog_sources_have_file_group_and_correct_target_membership(self):
        project = (IOS / "Synveil.xcodeproj/project.pbxproj").read_text()
        objects = re.findall(r"^\s*([A-F0-9]{24}) /\*.*?\*/ =", project, re.M)
        self.assertEqual(len(objects), len(set(objects)))
        sources = re.findall(r"isa = PBXSourcesBuildPhase;.*?files = \((.*?)\);", project, re.S)
        groups = re.findall(r"isa = PBXGroup;.*?children = \((.*?)\);", project, re.S)
        for path in [
            "Domain/Library/LibraryModels.swift",
            "Domain/Library/LibraryResponseDTO.swift",
            "Application/Library/AuthenticatedLibraryRequestProvider.swift",
            "Application/Library/AuthenticatedLibraryCatalogRepository.swift",
            "Tests/SynveilTests/LibraryCatalogTests.swift",
        ]:
            self.assertTrue((IOS / path).is_file())
            reference = re.search(r"([A-F0-9]{24}) /\*.*?\*/ = \{isa = PBXFileReference;[^\n]*path = " + re.escape(path) + r";", project)
            self.assertIsNotNone(reference, path)
            ref_id = reference.group(1)
            self.assertEqual(sum(ref_id in group for group in groups), 1)
            build = re.search(r"([A-F0-9]{24}) /\*.*?\*/ = \{isa = PBXBuildFile; fileRef = " + ref_id, project)
            self.assertIsNotNone(build)
            target = 1 if path.startswith("Tests/") else 0
            self.assertIn(build.group(1), sources[target])
            self.assertNotIn(build.group(1), sources[1 - target])

    def test_production_catalog_reuses_security_and_bounded_transport(self):
        composition = (IOS / "App/AppDependencyContainer.swift").read_text()
        self.assertEqual(composition.count("KeychainCredentialStore(rustBridge: bridge)"), 1)
        self.assertIn("maxResponseBodyBytes: LibraryCatalogPolicy.maximumResponseBytes", composition)
        self.assertIn("public private(set) var libraryCatalog:", composition)
        provider = (IOS / "Application/Library/AuthenticatedLibraryRequestProvider.swift").read_text()
        self.assertIn("store.load(expectedServerEndpoint: endpoint)", provider)
        self.assertNotIn("SecItem", provider)
        self.assertNotIn("store.delete", provider)


if __name__ == "__main__":
    unittest.main()
