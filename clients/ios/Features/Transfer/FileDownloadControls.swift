import SwiftUI

struct FileDownloadControls: View {
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
                Text(model.label).accessibilityAddTraits(.updatesFrequently)
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
                if let failure = model.failure {
                    Text(failure.message).fixedSize(horizontal: false, vertical: true)
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
            .onChange(of: controller.lifecycleRevision) { _, _ in model.invalidate() }
            .onDisappear { model.invalidate() }
        }
        FileExportControls(
            coordinator: exportCoordinator, controller: controller,
            library: library, node: node, version: version, displayName: displayName,
            verified: model.progress?.state == .verified)
    }
}
