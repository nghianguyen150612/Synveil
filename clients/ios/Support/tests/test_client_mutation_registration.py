"""Mutation target membership and the fail-closed production composition boundary."""
import re
import unittest
from pathlib import Path

IOS = Path(__file__).resolve().parents[2]


class ClientMutationRegistrationTests(unittest.TestCase):
    def test_mutation_sources_have_exact_target_membership(self):
        project = (IOS / "Synveil.xcodeproj/project.pbxproj").read_text()
        objects = re.findall(r"^\s*([A-F0-9]{24}) /\*.*?\*/ =", project, re.M)
        self.assertEqual(len(objects), len(set(objects)))
        sources = re.findall(r"isa = PBXSourcesBuildPhase;.*?files = \((.*?)\);", project, re.S)
        groups = re.findall(r"isa = PBXGroup;.*?children = \((.*?)\);", project, re.S)
        for path in [
            "Domain/Mutation/ClientMutationModels.swift",
            "Domain/Mutation/ClientMutationResponseDTO.swift",
            "Application/Mutation/AuthenticatedClientMutationRepository.swift",
            "Tests/SynveilTests/ClientMutationTests.swift",
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

    def test_production_repository_has_no_durable_authorizer_or_public_write_interface(self):
        composition = (IOS / "App/AppDependencyContainer.swift").read_text()
        repository = (IOS / "Application/Mutation/AuthenticatedClientMutationRepository.swift").read_text()
        self.assertIn("clientMutationRepository = AuthenticatedClientMutationRepository(", composition)
        self.assertIn("maxResponseBodyBytes: ClientMutationPolicy.maximumResponseBytes", composition)
        self.assertNotIn("authorizer:", composition)
        self.assertNotIn("public func submit", repository)
        self.assertIn("guard let authorizer else", repository)
        self.assertIn(".failed(.preparationRequired)", repository)
        self.assertEqual(composition.count("KeychainCredentialStore(rustBridge: bridge)"), 1)

    def test_native_browsing_has_no_mutation_dependency(self):
        for path in (IOS / "Features").rglob("*.swift"):
            text = path.read_text()
            for symbol in ["clientMutationRepository", "PreparedClientMutation", "ClientMutationPreparationAuthorizer"]:
                self.assertNotIn(symbol, text, str(path))
        provider = (IOS / "Application/Library/AuthenticatedLibraryRequestProvider.swift").read_text()
        self.assertEqual(provider.count('"Authorization":'), 1)
        self.assertIn('/api/v1/devices/\\(scope.session.record.deviceId)/libraries/\\(mutation.base.scope.libraryId.rawValue)/mutations', provider)
        self.assertNotIn("store.delete", provider)


if __name__ == "__main__":
    unittest.main()
