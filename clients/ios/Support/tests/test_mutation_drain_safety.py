"""P036 trusted invocation boundary and production capability composition."""
import unittest
from pathlib import Path

IOS = Path(__file__).resolve().parents[2]


class MutationDrainSafetyTests(unittest.TestCase):
    def test_composition_shares_credentials_and_global_repository_remains_gated(self):
        text = (IOS / "App/AppDependencyContainer.swift").read_text()
        self.assertEqual(text.count("KeychainCredentialStore(rustBridge: bridge)"), 1)
        self.assertIn("provider: mutationProvider, bridge: bridge)", text)
        self.assertNotIn("authorizer:", text)
        self.assertIn("queue: queue, provider: mutationProvider, bridge: bridge", text)
        self.assertIn("mutationDrainCoordinator = nil", text)

    def test_no_automatic_or_presentation_invocation(self):
        for folder in ["App", "Features"]:
            for path in (IOS / folder).rglob("*.swift"):
                text = path.read_text()
                calls = [".drain(", ".submitMutation("]
                if folder == "Features": calls += ["clientMutationRepository", "MutationSubmissionLease"]
                for call in calls:
                    self.assertNotIn(call, text, str(path))
                if path.name == "MetadataMutationViewModel.swift":
                    # The explicit user-confirmed unknown action may cross the application facade;
                    # SwiftUI never calls the coordinator or transport itself.
                    self.assertIn("feature.reconcileUnknown(mutationId: mutationId, in: library)", text)
                elif path.name == "MetadataMutationView.swift":
                    self.assertIn('"Check / Retry Original Operation"', text)
                    self.assertIn("viewModel.reconcileUnknown(mutationId: id)", text)
                else:
                    self.assertNotIn(".reconcileUnknown(", text, str(path))
        text = (IOS / "Application/Mutation/MutationDrainCoordinator.swift").read_text()
        for symbol in ["Timer(", "BGTask", "UUID(", "URLSession", "NodeRepository", "SyncCheckpointService"]:
            self.assertNotIn(symbol, text)

    def test_repository_is_per_lease_and_transport_cannot_capture_new_session(self):
        text = (IOS / "Application/Mutation/MutationDrainCoordinator.swift").read_text()
        self.assertIn("DurableMutationAttemptAuthorizer(queue: queue, lease: lease)", text)
        self.assertIn("authorizer: authorizer", text)
        self.assertIn("session: lease.session", text)
        self.assertIn("repository.submit(lease.mutation)", text)
        self.assertNotIn("provider.begin()", text)
        self.assertIn("queue.acquireRecoveryAttempt", text)
        self.assertIn("queue.acquireAttempt", text)
        self.assertIn("maximumReadBatch", text)
        self.assertRegex(text, r"queue\.finish\(\s*lease")


if __name__ == "__main__":
    unittest.main()
