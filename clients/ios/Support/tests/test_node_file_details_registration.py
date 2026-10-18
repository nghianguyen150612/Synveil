"""Protect the live-details composition and Xcode target membership."""
import re
import unittest
from pathlib import Path

IOS = Path(__file__).resolve().parents[2]


class NodeFileDetailsRegistrationTests(unittest.TestCase):
    def test_details_sources_have_exact_target_membership(self):
        project = (IOS / "Synveil.xcodeproj/project.pbxproj").read_text()
        objects = re.findall(r"^\s*([A-F0-9]{24}) /\*.*?\*/ =", project, re.M)
        self.assertEqual(len(objects), len(set(objects)))
        sources = re.findall(r"isa = PBXSourcesBuildPhase;.*?files = \((.*?)\);", project, re.S)
        groups = re.findall(r"isa = PBXGroup;.*?children = \((.*?)\);", project, re.S)
        for path in [
            "Features/Node/NodeFileDetailsView.swift",
            "Features/Node/NodeFileDetailsViewModel.swift",
            "Tests/SynveilTests/NodeFileDetailsViewModelTests.swift",
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

    def test_details_reuse_native_navigation_and_injected_node_repository(self):
        catalog = (IOS / "Features/Library/LibraryCatalogView.swift").read_text()
        view = (IOS / "Features/Node/NodeFileDetailsView.swift").read_text()
        model = (IOS / "Features/Node/NodeFileDetailsViewModel.swift").read_text()
        self.assertIn("repository: nodeRepository", catalog)
        self.assertIn("NodeFileDetailsView(", catalog)
        self.assertIn(".id(route)", catalog)
        self.assertNotIn("NavigationStack", view)
        self.assertIn("@State private var viewModel", view)
        self.assertIn("repository.getNode(", model)
        self.assertNotIn("listChildren(", model)
        self.assertIn(".onDisappear { viewModel.invalidate() }", view)
        self.assertIn(".onChange(of: sessionController.lifecycleRevision)", view)
        self.assertIn("switch viewModel.presentationState", view)
        self.assertIn("sessionController.lifecycleRevision == sessionRevision", model)

    def test_visible_metadata_comes_from_verified_node_not_route_snapshot(self):
        view = (IOS / "Features/Node/NodeFileDetailsView.swift").read_text()
        for snapshot in ["route.name", "route.createdAt", "route.updatedAt", "route.revision"]:
            self.assertNotIn(snapshot, view)
        for field in ["node.name", "node.createdAt", "node.updatedAt", "node.revision.rawValue", "node.currentVersionId"]:
            self.assertIn(field, view)
        self.assertIn("previously loaded metadata", view)
        self.assertIn("it has not been verified by this refresh", view)
        self.assertIn("This file is no longer available at its previous location.", view)
        for identifier in ["loading", "refresh", "name", "unavailable", "refresh-error", "error", "cancelled"]:
            self.assertIn("synveil.node.details." + identifier, view)
        self.assertIn(".textSelection(.enabled)", view)
        self.assertIn(".fixedSize(horizontal: false, vertical: true)", view)

    def test_single_and_collection_decoder_share_resource_validation(self):
        decoder = (IOS / "Domain/Node/NodeResponseDTO.swift").read_text()
        self.assertEqual(decoder.count("private func mapResource("), 1)
        self.assertEqual(decoder.count("private func validateResourceShape("), 1)
        self.assertEqual(decoder.count("bridge.validateLogicalName("), 1)
        self.assertIn("mapResource(dto.data)", decoder)
        self.assertIn("mapResource(item)", decoder)
        self.assertIn("decodeSingle(_ body: Data)", decoder)

    def test_details_add_no_credentials_transport_cache_or_file_actions(self):
        text = "\n".join((IOS / path).read_text() for path in [
            "Features/Node/NodeFileDetailsView.swift",
            "Features/Node/NodeFileDetailsViewModel.swift",
        ])
        for forbidden in ["svd1_", "URLSession", "UserDefaults", "KeychainCredentialStore", "AuthenticatedLibraryRequestProvider", "QuickLook", "ShareLink", "SwiftData", "CoreData"]:
            self.assertNotIn(forbidden, text)
        for operation in ["Download", "Upload", "Rename", "Delete", "Restore", "Move"]:
            self.assertNotRegex(text, r'Button\("' + operation)
        provider = (IOS / "Application/Library/AuthenticatedLibraryRequestProvider.swift").read_text()
        self.assertIn('path: "/api/v1/nodes/\\(nodeId.rawValue)", queryItems: nil', provider)
        self.assertEqual(provider.count('"Authorization":'), 1)


if __name__ == "__main__":
    unittest.main()
