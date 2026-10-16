import SwiftUI

struct FileDownloadControls: View {
    @Environment(\.scenePhase) private var scenePhase
    private let controller: SessionController
    private let library: LibraryId
    private let node: NodeId
    private let version: FileVersionId
    private let displayName: String
    private let exportCoordinator: VerifiedFileExportCoordinator?
    @State private var model: FileDownloadViewModel
    init(
        coordinator: FileDownloadCoordinator?, controller: SessionController,
        library: LibraryId, node: NodeId, version: FileVersionId,
        displayName: String = "Synveil Download",
        exportCoordinator: VerifiedFileExportCoordinator? = nil
    ) {
        self.controller = controller
        self.library = library
        self.node = node
        self.version = version
        self.displayName = displayName
        self.exportCoordinator = exportCoordinator
        _model = State(
            initialValue: FileDownloadViewModel(
                coordinator: coordinator,
                controller: controller, library: library, node: node, version: version))
    }
    var body: some View {
        if model.isCurrent {
            Section("Private Download") {
                Text(model.label(isSceneActive: scenePhase == .active))
                    .accessibilityAddTraits(
                        model.isLiveOperation(isSceneActive: scenePhase == .active)
                            ? .updatesFrequently : []
                    )
                    .accessibilityIdentifier("synveil.download.recovery.status")
                if let progress = model.progress {
                    if progress.totalBytes > 0 {
                        ProgressView(
                            value: Double(progress.durableBytes), total: Double(progress.totalBytes)
                        )
                        .accessibilityLabel("Durably downloaded bytes")
                        .accessibilityValue(
                            "\(progress.durableBytes) of \(progress.totalBytes) bytes. \(model.label)"
                        )
                    }
                    Text("\(progress.durableBytes) of \(progress.totalBytes) bytes stored")
                }
                if let message = model.savedStatusUnavailableMessage {
                    Text(message)
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("synveil.download.saved-status.unavailable")
                }
                if let failure = model.failure {
                    ClientRecoveryNotice(presentation: .download(failure))
                } else if let progress = model.progress,
                    let recovery = ClientRecoveryPresentation.download(
                        progress, isActive: model.isRunning)
                {
                    ClientRecoveryNotice(presentation: recovery)
                }
                if model.canStart {
                    Button("Download File") { Task { await model.download() } }
                        .accessibilityHint(
                            "Checks this version with the server and verifies a private local copy."
                        )
                        .accessibilityIdentifier("synveil.download.start")
                } else if model.canResume {
                    Button("Resume") { Task { await model.download(resume: true) } }
                        .accessibilityHint(
                            "Continues the original version after validating saved progress."
                        )
                        .accessibilityIdentifier("synveil.download.resume")
                }
                Button("Download in Background") {}
                    .disabled(true)
                    .accessibilityHint(BackgroundDownloadCapability.current.userMessage)
                    .accessibilityIdentifier("synveil.download.background.unavailable")
                Text(BackgroundDownloadCapability.current.userMessage)
                    .font(.footnote).fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("synveil.download.background.reason")
                if model.isRunning {
                    Button("Cancel", role: .cancel) { model.cancel() }
                        .accessibilityIdentifier("synveil.download.cancel")
                }
                if model.canRestart {
                    Button("Restart Download") { Task { await model.download(restart: true) } }
                        .accessibilityHint(
                            "Discards the incomplete cache and retries the same file version from zero."
                        )
                        .accessibilityIdentifier("synveil.download.restart")
                }
                Text(
                    "Private cached content may be removed by the system. Preview and export actions appear only for a verified local copy."
                )
                .font(.footnote).fixedSize(horizontal: false, vertical: true)
            }
            .task { await model.loadSaved() }
            .onChange(of: scenePhase) { _, phase in
                if phase == .active { Task { await model.loadSaved() } }
            }
            .onChange(of: controller.lifecycleRevision) { _, _ in model.invalidate() }
            .onDisappear { model.invalidate() }
        }
        FileExportControls(
            coordinator: exportCoordinator, controller: controller,
            library: library, node: node, version: version, displayName: displayName,
            verified: model.progress?.state == .verified)
    }
}
