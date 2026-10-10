"""P039 source membership, shared-connection authority and explicit-execution boundaries."""
import re
import unittest
from pathlib import Path

IOS = Path(__file__).resolve().parents[2]


class NodeProjectionFoundationTests(unittest.TestCase):
    def test_new_sources_registered_in_correct_targets(self):
        project = (IOS / "Synveil.xcodeproj/project.pbxproj").read_text()
        objects = re.findall(r"^\s*([A-F0-9]{24}) /\*.*?\*/ =", project, re.M)
        self.assertEqual(len(objects), len(set(objects)))
        phases = re.findall(r"isa = PBXSourcesBuildPhase;.*?files = \((.*?)\);", project, re.S)
        for path in [
            "Domain/Sync/NodeProjectionModels.swift",
            "Application/Sync/SyncNodeMaterializer.swift",
            "Application/Sync/SyncFeedApplicationService.swift",
            "Infrastructure/Persistence/NodeProjectionSQLiteSchema.swift",
            "Infrastructure/Persistence/CommittedSQLiteSyncProjectionStorage.swift",
            "Infrastructure/Persistence/SQLiteNodeProjectionRepository.swift",
            "Tests/SynveilTests/NodeProjectionTestSupport.swift",
            "Tests/SynveilTests/NodeProjectionTests.swift",
            "Tests/SynveilTests/NodeProjectionSQLiteTests.swift",
            "Tests/SynveilTests/CommittedProjectionAckTests.swift",
        ]:
            self.assertTrue((IOS / path).is_file(), path)
            ref = re.search(r"([A-F0-9]{24}) /\*.*?\*/ = \{isa = PBXFileReference;[^\n]*path = " + re.escape(path) + r";", project)
            self.assertIsNotNone(ref, path)
            build = re.search(r"([A-F0-9]{24}) /\*.*?\*/ = \{isa = PBXBuildFile; fileRef = " + ref.group(1), project)
            target = int(path.startswith("Tests/"))
            self.assertIn(build.group(1), phases[target])
            self.assertNotIn(build.group(1), phases[1-target])

    def test_same_database_boundary_and_no_network_in_transaction(self):
        storage = (IOS / "Infrastructure/Persistence/CommittedSQLiteSyncProjectionStorage.swift").read_text()
        self.assertIn("database: MutationQueueSQLiteStore", storage)
        self.assertNotIn("sqlite3_open", storage)
        store = (IOS / "Infrastructure/Persistence/MutationQueueSQLiteStore.swift").read_text()
        block = store.split("let existing = try transaction {", 1)[1].split("committed = true", 1)[0]
        self.assertNotIn("await ", block)
        self.assertIn("requireProjectionCommit", block)
        self.assertIn("APPLIED_ACK_PENDING", block)
        self.assertIn("projection_events", block)

    def test_no_automatic_ack_polling_or_cache_browser_replacement(self):
        composition = (IOS / "App/AppDependencyContainer.swift").read_text()
        for symbol in [".apply(scope:", ".acknowledge(", ".recoveryReceipt(", ".readAndStage("]:
            self.assertNotIn(symbol, composition)
        service = (IOS / "Application/Sync/SyncFeedApplicationService.swift").read_text()
        for symbol in ["Timer(", "BGAppRefreshTask", ".acknowledge(", "submitMutation(", "Task.detached"]:
            self.assertNotIn(symbol, service)
        for path in (IOS / "Features").rglob("*.swift"):
            self.assertNotIn("SQLiteNodeProjectionRepository", path.read_text())

    def test_projection_gate_requires_transaction_and_durable_evidence(self):
        source = (IOS / "Infrastructure/Persistence/NodeProjectionSQLiteSchema.swift").read_text()
        for symbol in ["synveil_projection_authorized()=1", "synveil_projection_authorized()=2", "synveil_projection_authorized()=3", "projection_commits", "projection_events", "purge_resurrection_gate", "inbound_insert_gate"]:
            self.assertIn(symbol, source)
        store = (IOS / "Infrastructure/Persistence/MutationQueueSQLiteStore.swift").read_text()
        self.assertIn("try verifySchema(Self.version3Schema)", store)
        self.assertIn('"DROP TRIGGER inbound_application_gate"', store)
        self.assertIn("sqlite3_create_function_v2", store)

    def test_partial_cache_never_publishes_authoritative_empty_directory(self):
        store = (IOS / "Infrastructure/Persistence/MutationQueueSQLiteStore.swift").read_text()
        self.assertIn("row.completeness != .complete", store)
        self.assertIn(".missing : .partial", store)
        models = (IOS / "Domain/Sync/NodeProjectionModels.swift").read_text()
        self.assertIn("provenance == .canonical", models)
        self.assertIn("metadata.revision.rawValue == revision.rawValue", models)

    def test_native_file_backed_projection_and_ack_cases_exist(self):
        support = (IOS / "Tests/SynveilTests/MutationQueueTestSupport.swift").read_text()
        self.assertIn("FileManager.default.temporaryDirectory", support)
        sqlite = (IOS / "Tests/SynveilTests/NodeProjectionSQLiteTests.swift").read_text()
        for symbol in ["projectionDowngradeV3", "sqlite3_open", "afterFirstNodeWrite", "cached_nodes", "projection_commits"]:
            self.assertIn(symbol, sqlite)
        ack = (IOS / "Tests/SynveilTests/CommittedProjectionAckTests.swift").read_text()
        self.assertIn("projectionStorage(f)", ack)
        self.assertIn("recoveryReceipt", ack)
        self.assertNotIn("AppliedFeedFixture", ack)


if __name__ == "__main__":
    unittest.main()
