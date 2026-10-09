"""P035 source membership, system SQLite linkage, and production dispatch gate."""
import re
import unittest
from pathlib import Path

IOS = Path(__file__).resolve().parents[2]


class DurableMutationQueueRegistrationTests(unittest.TestCase):
    def test_all_p035_sources_have_exact_target_membership(self):
        project = (IOS / "Synveil.xcodeproj/project.pbxproj").read_text()
        objects = re.findall(r"^\s*([A-F0-9]{24}) /\*.*?\*/ =", project, re.M)
        self.assertEqual(len(objects), len(set(objects)))
        sources = re.findall(r"isa = PBXSourcesBuildPhase;.*?files = \((.*?)\);", project, re.S)
        groups = re.findall(r"isa = PBXGroup;.*?children = \((.*?)\);", project, re.S)
        for path in [
            "Domain/Mutation/SyncCheckpointModels.swift",
            "Domain/Mutation/MutationQueueModels.swift",
            "Domain/Mutation/SyncCheckpointResponseDTO.swift",
            "Domain/Mutation/MutationPersistenceCodec.swift",
            "Application/Mutation/SyncCheckpointService.swift",
            "Application/Mutation/DurableMutationQueue.swift",
            "Infrastructure/Persistence/MutationQueueSQLiteStore.swift",
            "Tests/SynveilTests/MutationQueueTestSupport.swift",
            "Tests/SynveilTests/MutationQueueSQLiteTests.swift",
            "Tests/SynveilTests/SyncCheckpointTests.swift",
            "Tests/SynveilTests/DurableMutationQueueTests.swift",
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
        self.assertEqual(project.count('"-lsqlite3",'), 4)

    def test_checkpoint_is_explicit_and_queue_failure_preserves_browsing(self):
        composition = (IOS / "App/AppDependencyContainer.swift").read_text()
        self.assertRegex(composition, r"SyncCheckpointService\(\s*provider: browserProvider, queue: queue, bridge: bridge\)")
        self.assertNotIn("service.prepare(", composition)
        self.assertIn("mutationQueueFailure = DurableMutationQueue.classify(error)", composition)
        self.assertIn("queue.recoverInterruptedOperations()", composition)
        self.assertNotIn("authorizer:", composition)
        self.assertNotIn("DurableMutationAttemptAuthorizer", composition)
        self.assertEqual(composition.count("KeychainCredentialStore(rustBridge: bridge)"), 1)
        for path in (IOS / "Features").rglob("*.swift"):
            text = path.read_text()
            for symbol in ["DurableMutationQueue", "SyncCheckpointService", "MutationSubmissionLease", "ClientMutationIntent"]:
                self.assertNotIn(symbol, text, str(path))

    def test_no_worker_timer_or_post_in_queue(self):
        for name in ["DurableMutationQueue", "SyncCheckpointService"]:
            source = (IOS / f"Application/Mutation/{name}.swift").read_text()
            for symbol in ["Timer(", "BGTask", "submitMutation(", ".submit(", "URLSession"]:
                self.assertNotIn(symbol, source)
        controller = (IOS / "Application/Session/SessionController.swift").read_text()
        self.assertIn("if state == .authenticated { mutationSessionInvalidator?() }", controller)


if __name__ == "__main__":
    unittest.main()
