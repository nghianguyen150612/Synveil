import Foundation
import Observation

@Observable
@MainActor
final class FileDownloadViewModel {
    private(set) var progress: DownloadProgress?
    private(set) var failure: DownloadFailure?
    private(set) var isRunning = false
    private(set) var isPreparing = false
    private let coordinator: FileDownloadCoordinator?
    private let controller: SessionController
    private let revision: UInt64
    private let library: LibraryId
    private let node: NodeId
    private let version: FileVersionId
    @ObservationIgnored private var task: Task<FileDownloadResult, Never>?
    @ObservationIgnored private var invalidated = false

    init(
        coordinator: FileDownloadCoordinator?, controller: SessionController,
        library: LibraryId, node: NodeId, version: FileVersionId
    ) {
        self.coordinator = coordinator
        self.controller = controller
        self.library = library
        self.node = node
        self.version = version
        revision = controller.lifecycleRevision
    }
    var isCurrent: Bool {
        !invalidated && controller.state == .authenticated
            && controller.lifecycleRevision == revision
    }
    var canStart: Bool { isCurrent && coordinator != nil && !isRunning && progress == nil }
    var canResume: Bool { isCurrent && !isRunning && progress?.state.canResume == true }
    var canRestart: Bool {
        isCurrent && !isRunning
            && (progress?.state == .failedPermanent || progress?.state == .integrityFailure)
    }
    var label: String {
        guard isCurrent else { return "Session changed" }
        if isPreparing { return "Preparing" }
        return progress?.state.label ?? "Download File"
    }
    func loadSaved() async {
        guard let coordinator, isCurrent, !isRunning, progress == nil else { return }
        do {
            let record = try await coordinator.saved(library: library, version: version)
            guard isCurrent, !isRunning else { return }
            progress = record.map(DownloadProgress.init)
            failure = record?.failure
        } catch {
            if isCurrent { failure = DownloadPolicy.classify(error) }
        }
    }
    func download(resume: Bool = false, restart: Bool = false) async {
        guard let coordinator, restart ? canRestart : (resume ? canResume : canStart) else {
            return
        }
        isRunning = true
        isPreparing = true
        failure = nil
        let operation = (resume || restart) ? progress?.operationId : nil
        let task = Task {
            await coordinator.run(
                library: library, node: node, selectedVersion: version, resume: operation,
                restart: restart
            ) {
                [weak self] progress in
                guard let self, self.isCurrent else { return }
                self.isPreparing = false
                self.progress = progress
            }
        }
        self.task = task
        let result = await withTaskCancellationHandler {
            await task.value
        } onCancel: {
            task.cancel()
        }
        self.task = nil
        isRunning = false
        isPreparing = false
        guard isCurrent else {
            progress = nil
            failure = nil
            return
        }
        if case .stopped(let record, let failure) = result {
            if let record { progress = DownloadProgress(record) }
            self.failure = failure
        }
    }
    func cancel() { task?.cancel() }
    func invalidate() {
        invalidated = true
        task?.cancel()
        task = nil
        progress = nil
        failure = nil
    }
}
