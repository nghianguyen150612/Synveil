import SwiftUI

struct RebaselineProgressView: View {
    @Environment(\.scenePhase) private var scenePhase
    let sessionController: SessionController
    @State private var model: RebaselineViewModel
    @State private var confirmsStart = false
    @State private var confirmsHandoff = false
    init(
        library: Library, coordinator: RebaselineCoordinator?, sessionController: SessionController
    ) {
        self.sessionController = sessionController
        _model = State(
            initialValue: RebaselineViewModel(
                library: library, coordinator: coordinator, sessionController: sessionController))
    }
    var body: some View {
        Section("Rebuild Saved Metadata") {
            Text(model.message).fixedSize(horizontal: false, vertical: true)
                .accessibilityIdentifier("synveil.rebaseline.status")
            if let progress = model.progress {
                Text("\(progress.pages) pages saved")
                Text("\(progress.stagedNodes) of \(progress.expectedNodes) Nodes staged")
                    .accessibilityAddTraits(.updatesFrequently)
                    .accessibilityIdentifier("synveil.rebaseline.count")
            }
            if model.isBusy {
                ProgressView("Rebuilding saved metadata…")
                Button("Cancel Snapshot Operation", role: .cancel) { model.cancel() }
            }
            Button("Rebuild Saved Metadata") { confirmsStart = true }
                .disabled(!model.canStart)
                .accessibilityIdentifier("synveil.rebaseline.start")
            if let action = model.action {
                switch action {
                case .resumeDownload:
                    Button("Continue Snapshot Download") { Task { await model.continueSnapshot() } }
                case .prepare:
                    Button("Verify and Prepare Snapshot") {
                        Task { await model.continueSnapshot() }
                    }
                case .complete, .recoverOriginalCompletion, .activateConfirmed:
                    Button(
                        action == .complete
                            ? "Confirm Checkpoint and Apply Snapshot"
                            : "Recover Original Snapshot Completion"
                    ) { confirmsHandoff = true }
                    .accessibilityIdentifier("synveil.rebaseline.recover")
                case .repeatStart: EmptyView()
                }
            }
        }
        .task { await model.loadStatus() }
        .onDisappear { model.cancel() }
        .onChange(of: scenePhase) { _, phase in if phase != .active { model.cancel() } }
        .onChange(of: sessionController.lifecycleRevision) { _, _ in model.sessionDidChange() }
        .confirmationDialog(
            "Rebuild saved metadata?", isPresented: $confirmsStart, titleVisibility: .visible
        ) {
            Button("Confirm Snapshot Start") { Task { await model.start(confirmedByUser: true) } }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text(
                "This requests an authenticated server snapshot. Your current partial cache stays available while the replacement is staged. File contents are not downloaded."
            )
        }
        .confirmationDialog(
            "Complete or recover the snapshot handoff?", isPresented: $confirmsHandoff,
            titleVisibility: .visible
        ) {
            Button("Confirm Snapshot Handoff") {
                Task { await model.continueSnapshot(confirmedByUser: true) }
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text(
                "This may advance the server checkpoint. Recovery uses the original saved token once. The prepared snapshot becomes active only after verified server completion and a local database commit."
            )
        }
    }
}
