"""P042 native registration and snapshot security boundaries."""
import re
import unittest
from pathlib import Path

IOS = Path(__file__).resolve().parents[2]


class RebaselineSnapshotArchitectureTests(unittest.TestCase):
    def test_snapshot_sources_have_unique_correct_target_membership(self):
        project = (IOS / "Synveil.xcodeproj/project.pbxproj").read_text()
        phases = re.findall(r"isa = PBXSourcesBuildPhase;.*?files = \((.*?)\);", project, re.S)
        objects = re.findall(r"^\s*([A-F0-9]{24}) /\*.*?\*/ =", project, re.M)
        self.assertEqual(len(objects), len(set(objects)))
        sources = list(IOS.rglob("Rebaseline*.swift")) + [IOS / "Features/Node/SnapshotNodeInformationView.swift"]
        self.assertEqual(len(sources), 13)
        for source in sources:
            path = source.relative_to(IOS).as_posix()
            reference = re.search(r"([A-F0-9]{24}) /\*.*?\*/ = \{isa = PBXFileReference;[^\n]*path = " + re.escape(path) + ";", project)
            self.assertIsNotNone(reference, path)
            build = re.search(r"([A-F0-9]{24}) /\*.*?\*/ = \{isa = PBXBuildFile; fileRef = " + reference.group(1), project)
            target = int(path.startswith("Tests/"))
            self.assertIn(build.group(1), phases[target])
            self.assertNotIn(build.group(1), phases[1-target])

    def test_ui_has_no_completion_token_or_database_authority(self):
        for source in (IOS / "Features/Sync").glob("Rebaseline*.swift"):
            text = source.read_text()
            for forbidden in ["completion_token", "terminalToken", "RebaselineHandoff", "SQLite3", "MutationQueueSQLiteStore"]:
                self.assertNotIn(forbidden, text)

    def test_startup_composes_without_network_execution(self):
        container = (IOS / "App/AppDependencyContainer.swift").read_text()
        self.assertIn("RebaselineCoordinator(", container)
        self.assertIn("composedRebaseline?.invalidateSession()", container)
        for source in (IOS / "App").glob("*.swift"):
            for forbidden in [".startRebaseline(", ".completeRebaseline(", ".requestRebaselinePage("]:
                self.assertNotIn(forbidden, source.read_text())

    def test_snapshot_metadata_never_constructs_live_nodes(self):
        dto = (IOS / "Domain/Sync/RebaselineResponseDTO.swift").read_text()
        self.assertNotIn("return Node(", dto)
        self.assertIn("return RebaselineSnapshotNode(", dto)
        details = (IOS / "Features/Node/SnapshotNodeInformationView.swift").read_text()
        self.assertIn("timestamps and restoration metadata are unavailable", details)
        self.assertNotIn("QuickLook", details)

    def test_local_migration_and_handoff_guards_use_shared_database(self):
        source = (IOS / "Infrastructure/Persistence/MutationQueueSQLiteStore.swift").read_text()
        self.assertIn("static let schemaVersion = 5", source)
        self.assertIn("try verifySchema(Self.version4Schema)", source)
        self.assertIn("try requireNoRebaseline(proof.evidence.scope)", source)
        self.assertIn("try requireNoRebaseline(mutation.base.scope)", source)
        self.assertIn("beforeSnapshotActivationCommit", source)
        self.assertIn("beforeSnapshotConfirmationCommit", source)

    def test_progress_is_native_explicit_and_accessible(self):
        source = (IOS / "Features/Sync/RebaselineProgressView.swift").read_text()
        for expected in ["Rebuild Saved Metadata", "Continue Snapshot Download", "Recover Original Snapshot Completion", ".confirmationDialog(", ".updatesFrequently", ".fixedSize(horizontal: false, vertical: true)"]:
            self.assertIn(expected, source)
        self.assertIn(".task { await model.loadStatus() }", source)


if __name__ == "__main__":
    unittest.main()
