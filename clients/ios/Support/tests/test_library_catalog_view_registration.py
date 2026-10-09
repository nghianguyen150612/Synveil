"""Checks the authenticated Library UI, lifecycle fence, accessibility and Xcode targets."""
import re
import unittest
from pathlib import Path

IOS = Path(__file__).resolve().parents[2]
ROOT = IOS / "App/RootView.swift"
SCREEN = IOS / "Features/Library/LibraryCatalogView.swift"
VIEW_MODEL = IOS / "Features/Library/LibraryCatalogViewModel.swift"


class LibraryCatalogViewRegistrationTests(unittest.TestCase):
    def test_catalog_swift_files_are_registered_in_the_correct_targets(self):
        project = (IOS / "Synveil.xcodeproj/project.pbxproj").read_text()
        objects = re.findall(r"^\s*([A-F0-9]{24}) /\*.*?\*/ =", project, re.M)
        self.assertEqual(len(objects), len(set(objects)))
        source_phases = re.findall(
            r"isa = PBXSourcesBuildPhase;.*?files = \((.*?)\);", project, re.S
        )
        groups = re.findall(r"isa = PBXGroup;.*?children = \((.*?)\);", project, re.S)

        for path in [
            "Features/Library/LibraryCatalogViewModel.swift",
            "Features/Library/LibraryCatalogView.swift",
            "Tests/SynveilTests/LibraryCatalogViewModelTests.swift",
            "Tests/SynveilTests/LibraryCatalogViewTests.swift",
        ]:
            self.assertTrue((IOS / path).is_file(), path)
            reference = re.search(
                r"([A-F0-9]{24}) /\*.*?\*/ = \{isa = PBXFileReference;[^\n]*path = "
                + re.escape(path)
                + r";",
                project,
            )
            self.assertIsNotNone(reference, path)
            reference_id = reference.group(1)
            self.assertEqual(sum(reference_id in group for group in groups), 1)
            build = re.search(
                r"([A-F0-9]{24}) /\*.*?\*/ = \{isa = PBXBuildFile; fileRef = "
                + reference_id,
                project,
            )
            self.assertIsNotNone(build, path)
            target = 1 if path.startswith("Tests/") else 0
            self.assertIn(build.group(1), source_phases[target])
            self.assertNotIn(build.group(1), source_phases[1 - target])

    def test_only_authenticated_root_routes_to_catalog_with_injected_repository(self):
        root = ROOT.read_text()
        self.assertIn("case .authenticated:", root)
        self.assertIn("LibraryCatalogView(", root)
        self.assertIn("repository: libraryCatalog", root)
        self.assertIn("sessionController: sessionController", root)
        self.assertNotIn("AuthenticatedShellPlaceholderView", root)
        self.assertIn("libraryCatalog: container.libraryCatalog", (IOS / "App/SynveilApp.swift").read_text())

    def test_catalog_uses_native_loading_refresh_selection_and_read_only_details(self):
        screen = SCREEN.read_text()
        for token in [
            "NavigationStack",
            "List {",
            ".refreshable",
            "ProgressView(\"Loading libraries",
            "No libraries available",
            "no libraries visible to this device",
            "NavigationLink(value: library.id)",
            "LibraryReadOnlyDetailView",
            "Folder browsing is not available yet.",
            "Log Out and Forget Session",
            "confirmationDialog(",
            "READ_ONLY",
        ]:
            if token == "READ_ONLY":
                self.assertIn("case .readOnly", VIEW_MODEL.read_text())
            else:
                self.assertIn(token, screen)
        self.assertIn(".accessibilityIdentifier(\"synveil.library.loading\")", screen)
        self.assertIn(".accessibilityIdentifier(\"synveil.library.empty.message\")", screen)
        self.assertIn(".accessibilityAddTraits(.updatesFrequently)", screen)
        self.assertIn(".fixedSize(horizontal: false, vertical: true)", screen)
        self.assertIn("Text(library.name)", screen)
        self.assertIn("Label(status.label, systemImage: status.symbol)", screen)
        self.assertIn("case .readOnly", VIEW_MODEL.read_text())
        self.assertIn("case .quarantined", VIEW_MODEL.read_text())
        self.assertIn("SessionLogoutAccessibility.logoutButton", screen)
        self.assertIn(".disabled(!viewModel.canRefresh || viewModel.isRequestInProgress)", screen)
        self.assertNotIn(".frame(width:", screen)

    def test_model_uses_repository_and_fences_transient_data_to_session(self):
        model = VIEW_MODEL.read_text()
        self.assertIn("@Observable", model)
        self.assertIn("@MainActor", model)
        self.assertIn("LibraryCatalogRepositoryProtocol", model)
        self.assertIn("repository.listLibraries()", model)
        self.assertIn("sessionController.state == .authenticated", model)
        self.assertIn("sessionController.lifecycleRevision == sessionRevision", model)
        self.assertIn("func invalidate()", model)
        self.assertIn("case refreshFailed([Library], LibraryCatalogUIFailure)", model)
        self.assertNotRegex(model, r"\b(?:DeviceBearer|Bearer|svd1_)\b")

    def test_screen_does_not_add_network_cache_or_sync_implementations(self):
        feature = SCREEN.read_text() + VIEW_MODEL.read_text()
        for forbidden in [
            "URLSession",
            "UserDefaults",
            "SwiftData",
            "CoreData",
            "GRDB",
            "SQLite",
            "Sync Now",
            "Sync now",
            "download",
            "upload",
        ]:
            self.assertNotIn(forbidden, feature)


if __name__ == "__main__":
    unittest.main()
