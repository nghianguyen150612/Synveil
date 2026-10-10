"""P037 facade, explicit invocation and bounded activity invariants."""
import unittest
from pathlib import Path

IOS = Path(__file__).resolve().parents[2]


class MetadataMutationFeatureSafetyTests(unittest.TestCase):
    def read(self, relative):
        return (IOS / relative).read_text()

    def test_production_composition_injects_only_the_narrow_feature(self):
        app = self.read("App/AppDependencyContainer.swift")
        root = self.read("App/RootView.swift")
        swiftui_app = self.read("App/SynveilApp.swift")
        self.assertIn("metadataMutationFeature: (any MetadataMutationFeatureProtocol)?", app)
        self.assertIn("metadataMutationFeature: metadataMutationFeature", root)
        self.assertIn("metadataMutationFeature: container.metadataMutationFeature", swiftui_app)
        for source in [root, swiftui_app, self.read("Features/Library/LibraryCatalogView.swift")]:
            self.assertNotIn("AuthenticatedClientMutationRepository", source)
            self.assertNotIn("MutationQueueSQLiteStore", source)
            self.assertNotIn("MutationSubmissionLease", source)

    def test_queue_and_checkpoint_calls_have_explicit_user_invocation_paths(self):
        service = self.read("Application/Mutation/MetadataMutationService.swift")
        browser = self.read("Features/Node/NodeBrowserView.swift")
        activity = self.read("Features/Mutation/MetadataMutationView.swift")
        composition = self.read("App/AppDependencyContainer.swift")
        self.assertEqual(service.count("checkpointService.prepare(scope: scope)"), 1)
        self.assertEqual(service.count("coordinator.drain(scope: scope, maximumOperations: 10)"), 1)
        self.assertEqual(service.count("coordinator.reconcileUnknown(scope: scope, mutationId: id)"), 1)
        self.assertIn("Task { await mutationViewModel.enableChanges() }", browser)
        self.assertIn("Task { await viewModel.sendPendingChanges() }", activity)
        self.assertIn("Task { await viewModel.reconcileUnknown(mutationId: id) }", activity)
        self.assertNotIn("checkpoint.prepare(scope:", composition)
        self.assertNotIn("coordinator.drain(", browser + activity)
        self.assertNotIn("coordinator.reconcileUnknown(", browser + activity)

    def test_queue_activity_query_is_bounded_and_prioritizes_unresolved_rows(self):
        store = self.read("Infrastructure/Persistence/MutationQueueSQLiteStore.swift")
        queue = self.read("Application/Mutation/DurableMutationQueue.swift")
        service = self.read("Application/Mutation/MetadataMutationService.swift")
        self.assertIn('state NOT IN (\'APPLIED\',\'FAILED_PERMANENT\') ORDER BY enqueue_order LIMIT ?', store)
        self.assertIn("activityRecords(scope: scope, limit: limit)", queue)
        self.assertIn("MutationQueuePolicy.maximumReadBatch", service)
        self.assertIn("(outstanding + terminal).sorted { $0.order < $1.order }", store)

    def test_ui_maps_all_persisted_states_and_discloses_history_limit(self):
        model = self.read("Features/Mutation/MetadataMutationViewModel.swift")
        view = self.read("Features/Mutation/MetadataMutationView.swift")
        for state in [".pending", ".submitting", ".applied", ".conflict", ".outcomeUnknown", ".blockedRebaseline", ".failedPermanent"]:
            self.assertIn(f"case {state}:", model)
        self.assertIn("Showing up to 100 operations", view)
        self.assertIn("Unresolved changes are prioritized", view)

    def test_native_entry_points_are_gated_confirmed_and_accessible(self):
        browser = self.read("Features/Node/NodeBrowserView.swift")
        view = self.read("Features/Mutation/MetadataMutationView.swift")
        model = self.read("Features/Mutation/MetadataMutationViewModel.swift")
        self.assertIn("if mutationViewModel.canEdit", browser)
        self.assertIn("route.library.status != .active", browser)
        self.assertIn('"Move \\(pendingTrashNode?.name ?? "item") to Trash?"', browser)
        self.assertIn("nonempty folder may be rejected", browser)
        self.assertIn("MoveDestinationPickerView(", browser)
        self.assertIn("NavigationStack(path: $path)", view)
        self.assertIn("libraryIsWritable", view)
        self.assertIn("if item.mayCheckRestore && libraryIsWritable", view)
        self.assertIn('"Check / Retry Original Operation"', view)
        self.assertIn("MutationQueuePolicy.maximumRecoveryAttempts", model)
        for identifier in [
            "synveil.node.new-folder", "synveil.node.pending-changes",
            "synveil.node.trash.confirm", "synveil.mutation.send-pending",
            "synveil.mutation.unknown.confirm", "synveil.mutation.activity.error",
        ]:
            self.assertIn(identifier, browser + view)

    def test_metadata_controls_are_present_without_unsupported_content_operations(self):
        browser = self.read("Features/Node/NodeBrowserView.swift")
        activity = self.read("Features/Mutation/MetadataMutationView.swift")
        for label in ["New Folder", "Rename", "Move", "Move to Trash", "Pending Changes"]:
            self.assertIn(label, browser)
        for label in ["Send Pending Changes", "Check Unknown Outcome", "Conflict — review needed"]:
            self.assertIn(label, activity + self.read("Features/Mutation/MetadataMutationViewModel.swift"))
        for unsupported in ["QuickLookPreview(", "ShareLink(", "FileProvider", "URLSession"]:
            self.assertNotIn(unsupported, browser + activity)
        self.assertNotIn('Button("Resolve Conflict"', activity)

    def test_presentation_layer_has_no_raw_mutation_or_credential_surface(self):
        files = [
            IOS / "Features/Mutation/MetadataMutationViewModel.swift",
            IOS / "Features/Mutation/MetadataMutationView.swift",
            IOS / "Features/Node/NodeBrowserView.swift",
        ]
        forbidden = [
            "AuthenticatedClientMutationRepository", "ClientMutationRepositoryProtocol",
            "MutationQueueSQLiteStore", "MutationSubmissionLease", "DeviceCredential",
            "DeviceBearer", "svd1_", "requestBody", "Authorization",
        ]
        for path in files:
            text = path.read_text()
            for token in forbidden:
                self.assertNotIn(token, text, str(path))


if __name__ == "__main__":
    unittest.main()
