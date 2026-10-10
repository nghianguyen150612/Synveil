import SwiftUI

struct SyncStatusView: View {
    @Environment(\.scenePhase) private var scenePhase
    private let sessionController: SessionController
    @State private var viewModel: SyncStatusViewModel
    @State private var showsRecoveryConfirmation = false
    @State private var showsSetupConfirmation = false

    init(
        library: Library, coordinator: (any InboundSyncCoordinatorProtocol)?,
        projection: (any NodeProjectionRepositoryProtocol)?,
        checkpoint: (any SyncCheckpointPreparationProtocol)?, sessionController: SessionController
    ) {
        self.sessionController = sessionController
        _viewModel = State(
            initialValue: SyncStatusViewModel(
                library: library, coordinator: coordinator, projection: projection,
                checkpoint: checkpoint, sessionController: sessionController))
    }

    var body: some View {
        List {
            Section("Library") {
                Text(viewModel.library.name)
                    .font(.headline)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("synveil.sync.library")
                Text(viewModel.library.id.rawValue)
                    .font(.caption)
                    .textSelection(.enabled)
            }
            Section("Synchronization") {
                Text(viewModel.message)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityAddTraits(.updatesFrequently)
                    .accessibilityIdentifier("synveil.sync.status")
                if viewModel.isBusy {
                    ProgressView(SyncStatusViewModel.phaseLabel(viewModel.progress.phase))
                        .accessibilityLabel(
                            "Synchronization progress. \(SyncStatusViewModel.phaseLabel(viewModel.progress.phase))"
                        )
                        .accessibilityIdentifier("synveil.sync.progress")
                }
                Button("Sync Now") { Task { await viewModel.syncNow() } }
                    .disabled(!viewModel.canSync)
                    .accessibilityHint(
                        "Runs a bounded inbound metadata synchronization for this Library."
                    )
                    .accessibilityIdentifier("synveil.sync.now")
                if !viewModel.canSync {
                    Text(
                        viewModel.isBusy
                            ? "Wait for this operation or cancel it." : viewModel.message
                    )
                    .font(.footnote)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("synveil.sync.disabled-guidance")
                }
                if viewModel.state == .syncing {
                    Button("Cancel Synchronization", role: .cancel) { viewModel.cancel() }
                        .accessibilityIdentifier("synveil.sync.cancel")
                }
                if viewModel.state == .offline {
                    Button("Try Sync Again When Connected") {
                        Task { await viewModel.retryWhenConnectionAvailable() }
                    }
                    .accessibilityIdentifier("synveil.sync.offline-retry")
                }
                if viewModel.state == .checkpointRequired {
                    Button("Set Up Synchronization") { showsSetupConfirmation = true }
                        .disabled(!viewModel.canSetup)
                        .accessibilityIdentifier("synveil.sync.setup")
                }
                if viewModel.state == .unknownAck {
                    Button("Check / Retry Original Acknowledgment") {
                        showsRecoveryConfirmation = true
                    }
                    .disabled(!viewModel.canRecover)
                    .accessibilityIdentifier("synveil.sync.recover")
                    if viewModel.recovery?.canRecover == false {
                        Text(
                            "The eight-attempt acknowledgment limit has been reached. Further recovery requires reconciliation."
                        )
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("synveil.sync.recovery-limit")
                    }
                }
            }
            Section("Verified Progress") {
                Text("\(viewModel.progress.pagesProcessed) pages confirmed in this run")
                    .accessibilityIdentifier("synveil.sync.pages")
                Text("\(viewModel.progress.eventsApplied) events applied in this run")
                    .accessibilityIdentifier("synveil.sync.events")
                position("Locally applied", viewModel.progress.locallyApplied, id: "local")
                position("Server confirmed", viewModel.progress.serverConfirmed, id: "confirmed")
                position(
                    "Latest observed high watermark", viewModel.progress.observedHighWatermark,
                    id: "watermark")
                Text(
                    "The high watermark is an observation and may change. It does not prove local application."
                )
                .font(.footnote)
                .fixedSize(horizontal: false, vertical: true)
            }
            Section("Saved Metadata") {
                Text(viewModel.cacheMessage)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("synveil.sync.cache")
            }
        }
        .navigationTitle("Sync Status")
        .navigationBarTitleDisplayMode(.inline)
        .accessibilityIdentifier("synveil.sync.screen")
        .task { await viewModel.loadStatus() }
        .onDisappear { viewModel.cancel() }
        .onChange(of: scenePhase) { _, phase in
            if phase != .active { viewModel.cancel() }
        }
        .onChange(of: sessionController.lifecycleRevision) { _, _ in viewModel.sessionDidChange() }
        .confirmationDialog(
            "Retry the original acknowledgment?", isPresented: $showsRecoveryConfirmation,
            titleVisibility: .visible
        ) {
            Button("Confirm Original Acknowledgment Recovery") {
                Task { await viewModel.recoverAcknowledgement(confirmedByUser: true) }
            }
            .accessibilityIdentifier("synveil.sync.recover-confirm")
            Button("Cancel", role: .cancel) {}
                .accessibilityIdentifier("synveil.sync.recover-cancel")
        } message: {
            Text(
                "The server may already have accepted it. This sends the same saved acknowledgment once and checks the checkpoint. It does not create a new token."
            )
        }
        .confirmationDialog(
            "Set up synchronization?", isPresented: $showsSetupConfirmation,
            titleVisibility: .visible
        ) {
            Button("Confirm Synchronization Setup") {
                Task { await viewModel.prepareCheckpoint(confirmedByUser: true) }
            }
            .accessibilityIdentifier("synveil.sync.setup-confirm")
            Button("Cancel", role: .cancel) {}
        } message: {
            Text(
                "This contacts the server and may initialize this Device’s synchronization checkpoint for the Library. It does not download a complete snapshot."
            )
        }
    }

    private func position(_ title: String, _ value: SyncJournalPosition?, id: String) -> some View {
        VStack(alignment: .leading) {
            Text(title).font(.subheadline)
            Text(
                value.map { "Epoch \($0.epoch.rawValue), sequence \($0.sequence.rawValue)" }
                    ?? "Not verified"
            )
            .font(.body)
            .fixedSize(horizontal: false, vertical: true)
        }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("synveil.sync.position.\(id)")
    }
}
