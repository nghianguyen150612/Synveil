"""Checks production composition and Xcode registration for the Node boundary."""
import re
import unittest
from pathlib import Path

IOS = Path(__file__).resolve().parents[2]


class NodeBrowserRegistrationTests(unittest.TestCase):
    def test_node_sources_have_file_group_and_correct_target_membership(self):
        project = (IOS / "Synveil.xcodeproj/project.pbxproj").read_text()
        objects = re.findall(r"^\s*([A-F0-9]{24}) /\*.*?\*/ =", project, re.M)
        self.assertEqual(len(objects), len(set(objects)))
        sources = re.findall(r"isa = PBXSourcesBuildPhase;.*?files = \((.*?)\);", project, re.S)
        groups = re.findall(r"isa = PBXGroup;.*?children = \((.*?)\);", project, re.S)
        for path in [
            "Domain/Node/NodeModels.swift",
            "Domain/Node/NodeResponseDTO.swift",
            "Application/Node/AuthenticatedNodeRepository.swift",
            "Tests/SynveilTests/NodeBrowserTests.swift",
            "Features/Node/NodeBrowserViewModel.swift",
            "Features/Node/NodeBrowserView.swift",
            "Tests/SynveilTests/NodeBrowserViewModelTests.swift",
            "Tests/SynveilTests/NodeBrowserViewTests.swift",
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

    def test_native_browser_reuses_the_injected_repository_and_typed_scopes(self):
        root = (IOS / "App/RootView.swift").read_text()
        app = (IOS / "App/SynveilApp.swift").read_text()
        catalog = (IOS / "Features/Library/LibraryCatalogView.swift").read_text()
        browser = (IOS / "Features/Node/NodeBrowserView.swift").read_text()
        model = (IOS / "Features/Node/NodeBrowserViewModel.swift").read_text()

        self.assertIn("nodeRepository: container.nodeRepository", app)
        self.assertIn("nodeRepository: nodeRepository", root)
        self.assertIn("NodeBrowserView(", catalog)
        self.assertIn(".root(for: library)", catalog)
        self.assertIn("NavigationStack", catalog)
        self.assertIn("NavigationLink(value: destination)", browser)
        self.assertIn(".directory(node.id)", model)
        self.assertIn("NodeRepositoryProtocol", model)
        self.assertIn("repository.listChildren(libraryId: library.id, parent: parentScope)", model)
        self.assertIn("sessionController.lifecycleRevision == sessionRevision", model)
        self.assertIn("func invalidate()", model)
        self.assertIn(".refreshable", browser)
        self.assertIn("This folder is empty.", browser)
        self.assertIn("File content is not available in this version", (IOS / "Features/Node/NodeFileDetailsView.swift").read_text())
        self.assertIn('node.kind == .directory ? "folder" : "doc"', browser)
        self.assertIn("func accessibilityDescription(for node: Node)", browser)
        self.assertIn("navigationDestination(for: NodeBrowserRoute.self)", catalog)
        self.assertIn("navigationDestination(for: NodeFileDetailsRoute.self)", catalog)
        self.assertNotIn("Folder browsing is not available yet.", catalog)
        self.assertNotRegex(model, r"\b(?:DeviceBearer|Bearer|svd1_)\b")
        for supported_metadata_operation in [
            'Button("Rename"',
            'Button("Move"',
            'Label("Move to Trash"',
        ]:
            self.assertIn(supported_metadata_operation, browser)
        for fake_operation in [
            'Button("Download"',
            'Button("Upload"',
            'Button("Delete"',
            "ShareLink(",
            "QuickLookPreview(",
        ]:
            self.assertNotIn(fake_operation, browser)

    def test_browser_sources_do_not_add_transport_or_persistent_cache(self):
        browser = (IOS / "Features/Node/NodeBrowserView.swift").read_text()
        model = (IOS / "Features/Node/NodeBrowserViewModel.swift").read_text()
        for forbidden in [
            "URLSession",
            "UserDefaults",
            "SwiftData",
            "CoreData",
            "GRDB",
            "SQLite",
        ]:
            self.assertNotIn(forbidden, browser + model)

    def test_production_nodes_reuses_security_and_bounded_transport(self):
        composition = (IOS / "App/AppDependencyContainer.swift").read_text()
        self.assertEqual(composition.count("KeychainCredentialStore(rustBridge: bridge)"), 1)
        self.assertIn("maxResponseBodyBytes: LibraryCatalogPolicy.maximumResponseBytes", composition)
        self.assertIn("public private(set) var nodeRepository:", composition)
        self.assertIn("nodeRepository = AuthenticatedNodeRepository(provider: browserProvider, bridge: bridge)", composition)
        self.assertIn("provider: browserProvider, bridge: bridge", composition)
        provider = (IOS / "Application/Library/AuthenticatedLibraryRequestProvider.swift").read_text()
        self.assertIn("store.load(expectedServerEndpoint: endpoint)", provider)
        self.assertNotIn("SecItem", provider)
        self.assertNotIn("store.delete", provider)


if __name__ == "__main__":
    unittest.main()
