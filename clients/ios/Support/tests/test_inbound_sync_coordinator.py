"""P040 target registration and foreground-only security boundaries."""
import re
import unittest
from pathlib import Path

IOS = Path(__file__).resolve().parents[2]


class InboundSyncCoordinatorArchitectureTests(unittest.TestCase):
    def test_sources_are_unique_and_registered_in_correct_targets(self):
        project = (IOS / "Synveil.xcodeproj/project.pbxproj").read_text()
        phases = re.findall(r"isa = PBXSourcesBuildPhase;.*?files = \((.*?)\);", project, re.S)
        objects = re.findall(r"^\s*([A-F0-9]{24}) /\*.*?\*/ =", project, re.M)
        self.assertEqual(len(objects), len(set(objects)))
        paths = ["Domain/Sync/InboundSyncModels.swift", "Application/Sync/InboundSyncCoordinator.swift",
                 "Features/Sync/SyncStatusView.swift", "Features/Sync/SyncStatusViewModel.swift"]
        paths += [f"Tests/SynveilTests/{name}.swift" for name in
                  ["InboundSyncTestSupport", "InboundSyncCoordinatorTests", "InboundSyncRecoveryTests",
                   "InboundSyncRaceTests", "SyncStatusViewModelTests"]]
        for path in paths:
            self.assertTrue((IOS / path).is_file())
            ref = re.search(r"([A-F0-9]{24}) /\*.*?\*/ = \{isa = PBXFileReference;[^\n]*path = " + re.escape(path) + ";", project)
            self.assertIsNotNone(ref)
            build = re.search(r"([A-F0-9]{24}) /\*.*?\*/ = \{isa = PBXBuildFile; fileRef = " + ref.group(1), project)
            target = int(path.startswith("Tests/"))
            self.assertIn(build.group(1), phases[target])
            self.assertNotIn(build.group(1), phases[1-target])

    def test_ui_never_controls_sqlite_or_signed_receipts(self):
        for path in (IOS / "Features/Sync").glob("*.swift"):
            source = path.read_text()
            for forbidden in ["import SQLite3", "SyncAckService", "AppliedFeedCommitReceipt",
                              "MutationQueueSQLiteStore", "ack_token", "DeviceBearer", "receipt("]:
                self.assertNotIn(forbidden, source)
        coordinator = (IOS / "Application/Sync/InboundSyncCoordinator.swift").read_text()
        for forbidden in ["HTTPTransportRequest(", "URLSession", "ackBody(", "MutationDrainCoordinator", ".drain(", "while true", "Task.detached"]:
            self.assertNotIn(forbidden, coordinator)

    def test_app_composition_never_invokes_sync(self):
        for path in ["App/AppDependencyContainer.swift", "App/SynveilApp.swift", "App/RootView.swift"]:
            source = (IOS / path).read_text()
            for forbidden in [".synchronize(", ".recoverUnknownAcknowledgement(", ".readAndStage(", ".acknowledge("]:
                self.assertNotIn(forbidden, source)
        container = (IOS / "App/AppDependencyContainer.swift").read_text()
        self.assertIn("inboundSyncCoordinator = InboundSyncCoordinator(", container)
        catalog = (IOS / "Features/Library/LibraryCatalogView.swift").read_text()
        self.assertIn("SyncStatusView(", catalog)

    def test_status_view_uses_native_accessible_confirmed_actions(self):
        view = (IOS / "Features/Sync/SyncStatusView.swift").read_text()
        for symbol in ["List {", "ProgressView(", ".confirmationDialog(", "synveil.sync.now",
                       "synveil.sync.recover-confirm", "synveil.sync.setup-confirm",
                       "synveil.sync.position.", ".accessibilityLabel(", ".updatesFrequently",
                       ".fixedSize(horizontal: false, vertical: true)"]:
            self.assertIn(symbol, view)
        self.assertIn(".task { await viewModel.loadStatus() }", view)
        self.assertNotIn("Full offline copy ready", view)

    def test_storage_reads_are_bounded_scoped_and_do_not_migrate(self):
        source = (IOS / "Infrastructure/Persistence/MutationQueueSQLiteStore.swift").read_text()
        self.assertIn("static let schemaVersion = 5", source)
        self.assertIn("ORDER BY length(epoch),epoch,length(from_sequence),from_sequence LIMIT 1", source)
        self.assertIn("func claimInboundRun", source)
        self.assertIn('url.path + ".inbound.lock"', source)
        for method in ["oldestUnresolvedInbound", "latestConfirmedInbound", "inboundAckAttemptCount"]:
            body = source.split("func " + method, 1)[1].split("\n    func ", 1)[0]
            self.assertIn("requireFeedScope", body)
            self.assertIn("scope_id=?", body)

    def test_native_coordinator_tests_use_real_projection_authority(self):
        support = (IOS / "Tests/SynveilTests/InboundSyncTestSupport.swift").read_text()
        for symbol in ["AuthenticatedLibraryRequestProvider(", "SyncFeedApplicationService(",
                       "CommittedSQLiteSyncProjectionStorage(", "InboundSyncCoordinator("]:
            self.assertIn(symbol, support)
        self.assertNotIn("AppliedFeedFixture", support)
        races = (IOS / "Tests/SynveilTests/InboundSyncRaceTests.swift").read_text()
        self.assertIn("await gate.wait()", races)
        self.assertNotIn("sleep", races)


if __name__ == "__main__":
    unittest.main()
