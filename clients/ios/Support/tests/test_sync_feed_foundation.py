"""P038 source registration, capability composition and real SQLite schema guards."""
import re
import sqlite3
import unittest
from pathlib import Path

IOS = Path(__file__).resolve().parents[2]


class SyncFeedFoundationTests(unittest.TestCase):
    def test_source_target_membership(self):
        project = (IOS / "Synveil.xcodeproj/project.pbxproj").read_text()
        objects = re.findall(r"^\s*([A-F0-9]{24}) /\*.*?\*/ =", project, re.M)
        self.assertEqual(len(objects), len(set(objects)))
        phases = re.findall(r"isa = PBXSourcesBuildPhase;.*?files = \((.*?)\);", project, re.S)
        groups = re.findall(r"isa = PBXGroup;.*?children = \((.*?)\);", project, re.S)
        for path in [
            "Domain/Sync/SyncFeedModels.swift", "Domain/Sync/SyncFeedResponseDTO.swift",
            "Application/Sync/SyncFeedService.swift", "Application/Sync/SyncAckService.swift",
            "Tests/SynveilTests/SyncFeedTestSupport.swift", "Tests/SynveilTests/SyncFeedTests.swift",
            "Tests/SynveilTests/InboundSyncSQLiteTests.swift", "Tests/SynveilTests/SyncAckTests.swift",
        ]:
            self.assertTrue((IOS / path).is_file())
            reference = re.search(r"([A-F0-9]{24}) /\*.*?\*/ = \{isa = PBXFileReference;[^\n]*path = " + re.escape(path) + r";", project)
            self.assertIsNotNone(reference, path)
            ref = reference.group(1)
            self.assertEqual(sum(ref in group for group in groups), 1)
            build = re.search(r"([A-F0-9]{24}) /\*.*?\*/ = \{isa = PBXBuildFile; fileRef = " + ref, project)
            self.assertIsNotNone(build)
            target = int(path.startswith("Tests/"))
            self.assertIn(build.group(1), phases[target])
            self.assertNotIn(build.group(1), phases[1 - target])

    def test_production_ack_requires_committed_projection_authority(self):
        composition = (IOS / "App/AppDependencyContainer.swift").read_text()
        self.assertIn("syncFeedService = SyncFeedService(", composition)
        self.assertIn("SyncAckService(", composition)
        self.assertRegex(composition, r"CommittedSQLiteSyncProjectionStorage\(\s*database: database")
        self.assertNotIn("readAndStage(", composition)
        self.assertNotIn(".acknowledge(", composition)
        ack = (IOS / "Application/Sync/SyncAckService.swift").read_text()
        self.assertIn("fileprivate init(", ack)
        self.assertNotIn("#if DEBUG", ack)
        self.assertIn("projection.validateAppliedPage", ack)
        self.assertIn("projection.confirmAppliedPage", ack)
        self.assertIn("projection.authorizeAckDispatch", ack)
        for path in (IOS / "Features").rglob("*.swift"):
            self.assertNotIn("AppliedFeedCommitReceipt", path.read_text())
            self.assertNotIn("SyncFeedService", path.read_text())

    def test_no_automatic_execution_or_checkpoint_rewrite(self):
        feed = (IOS / "Application/Sync/SyncFeedService.swift").read_text()
        self.assertEqual(feed.count("provider.requestFeed("), 1)
        for symbol in ["while ", "Timer(", "BGTask", "submitMutation(", "persistCheckpoint(", "UserDefaults"]:
            self.assertNotIn(symbol, feed)
        provider = (IOS / "Application/Library/AuthenticatedLibraryRequestProvider.swift").read_text()
        self.assertRegex(provider, r"func submitSyncAck\(\s*_ receipt: AppliedFeedCommitReceipt")
        self.assertIn('URLQueryItem(name: "limit", value: String(limit))', provider)
        store = (IOS / "Infrastructure/Persistence/MutationQueueSQLiteStore.swift").read_text()
        self.assertIn("static let schemaVersion = 4", store)
        self.assertIn("try verifySchema(Self.version2Schema)", store)

    def test_actual_schema_rejects_ack_from_staging_and_retains_evidence(self):
        source = (IOS / "Infrastructure/Persistence/MutationQueueSQLiteStore.swift").read_text()
        block = source.split("static let inboundSchema = [", 1)[1].split("static let version3Schema", 1)[0]
        sql = re.findall(r'"""\n(.*?)\n\s*"""', block, re.S)
        self.assertEqual(len(sql), 3)
        db = sqlite3.connect(":memory:")
        self.addCleanup(db.close)
        db.execute("CREATE TABLE scopes(scope_id INTEGER PRIMARY KEY)")
        db.execute("INSERT INTO scopes VALUES(1)")
        for statement in sql:
            db.execute(statement)
        db.execute("INSERT INTO inbound_pages VALUES(1,'1','0','2','2',?, ?, 1,'RECEIVED_UNAPPLIED',1,1)", (b"response", b"canonical"))
        for state in ["APPLIED_ACK_PENDING", "ACK_IN_FLIGHT", "ACK_CONFIRMED"]:
            with self.assertRaises(sqlite3.IntegrityError):
                db.execute("UPDATE inbound_pages SET state=?", (state,))
        with self.assertRaises(sqlite3.IntegrityError):
            db.execute("UPDATE inbound_pages SET response=?", (b"replacement",))
        db.execute("UPDATE inbound_pages SET state='BLOCKED_REBASELINE'")
        self.assertEqual(db.execute("SELECT response,state FROM inbound_pages").fetchone(), (b"response", "BLOCKED_REBASELINE"))


if __name__ == "__main__":
    unittest.main()
