import Foundation
import Observation

@Observable
@MainActor
final class FileDownloadViewModel {
    private(set) var progress: DownloadProgress?
    private(set) var failure: DownloadFailure?
    private(set) var isRunning = false
    private(set) var isPreparing = false
    private(set) var isRefreshingSaved = false
    private let coordinator: FileDownloadCoordinator?
    private let controller: SessionController
    private let revision: UInt64
    private let library: LibraryId
    private let node: NodeId
    private let version: FileVersionId
    @ObservationIgnored private var task: Task<FileDownloadResult, Never>?
    @ObservationIgnored private var invalidated = false
    @ObservationIgnored private var activeSavedReadID: UUID?
    @ObservationIgnored private var activeOperationLifecycleGeneration: UInt64?

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
    var canStart: Bool {
        isCurrent && coordinator != nil && !isRunning && !isRefreshingSaved && progress == nil
    }
    var canResume: Bool {
        isCurrent && !isRunning && !isRefreshingSaved && progress?.state.canResume == true
    }
    var savedStatusUnavailableMessage: String? {
        guard isCurrent else { return nil }
        if coordinator == nil || failure?.state == .storageUnavailable {
            return "Saved transfer information is unavailable. Reopen Synveil after storage recovers."
        }
        return nil
    }
    var canRestart: Bool {
        isCurrent && !isRunning
            && (progress?.state == .failedPermanent || progress?.state == .integrityFailure)
    }
    var label: String {
        label(isSceneActive: true)
    }
    func label(isSceneActive: Bool) -> String {
        guard isCurrent else { return "Session changed" }
        if isRefreshingSaved { return "Checking saved transfer status…" }
        if isPreparing {
            if isRunning && !isLiveOperation(isSceneActive: isSceneActive) {
                return "Checking saved transfer status…"
            }
            return "Preparing"
        }
        guard let state = progress?.state else { return "Download File" }
        if isRunning {
            guard isLiveOperation(isSceneActive: isSceneActive) else {
                return "Checking saved transfer status…"
            }
            return state.label
        }
        switch state {
        case .prepared, .downloading, .partial, .cancelled, .failedRecoverable:
            return "Download interrupted — Resume available"
        case .verifying:
            return "Verification required"
        default:
            return state.label
        }
    }
    func isLiveOperation(isSceneActive: Bool) -> Bool {
        guard isSceneActive, isRunning,
            let coordinator,
            activeOperationLifecycleGeneration == coordinator.lifecycleGeneration
        else { return false }
        return true
    }
    func loadSaved() async {
        await readSaved(force: false)
    }

    private func readSaved(force: Bool, retryAfterLifecycleChange: Bool = true) async {
        guard let coordinator, isCurrent, !isRunning, !isRefreshingSaved,
            force || progress == nil
        else { return }
        let readID = UUID()
        let capturedLifecycleGeneration = coordinator.lifecycleGeneration
        activeSavedReadID = readID
        isRefreshingSaved = true
        defer {
            if activeSavedReadID == readID {
                activeSavedReadID = nil
                isRefreshingSaved = false
            }
        }
        do {
            let record = try await coordinator.saved(library: library, version: version)
            guard isCurrent, !isRunning,
                activeSavedReadID == readID
            else { return }
            guard coordinator.lifecycleGeneration == capturedLifecycleGeneration else {
                activeSavedReadID = nil
                isRefreshingSaved = false
                if retryAfterLifecycleChange, isCurrent {
                    await readSaved(force: true, retryAfterLifecycleChange: false)
                }
                return
            }
            progress = record.map(DownloadProgress.init)
            failure = record?.failure
        } catch {
            guard isCurrent, activeSavedReadID == readID else { return }
            guard coordinator.lifecycleGeneration == capturedLifecycleGeneration else {
                activeSavedReadID = nil
                isRefreshingSaved = false
                if retryAfterLifecycleChange, isCurrent {
                    await readSaved(force: true, retryAfterLifecycleChange: false)
                }
                return
            }
            failure = DownloadPolicy.classify(error)
        }
    }
    func download(resume: Bool = false, restart: Bool = false) async {
        guard let coordinator, restart ? canRestart : (resume ? canResume : canStart) else {
            return
        }
        isRunning = true
        isPreparing = true
        failure = nil
        let capturedLifecycleGeneration = coordinator.lifecycleGeneration
        activeOperationLifecycleGeneration = capturedLifecycleGeneration
        let operation = (resume || restart) ? progress?.operationId : nil
        let task = Task {
            await coordinator.run(
                library: library, node: node, selectedVersion: version, resume: operation,
                restart: restart
            ) {
                [weak self] progress in
                guard let self, self.isCurrent,
                    self.coordinator?.lifecycleGeneration == capturedLifecycleGeneration
                else { return }
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
        activeOperationLifecycleGeneration = nil
        guard isCurrent else {
            progress = nil
            failure = nil
            return
        }
        guard coordinator.lifecycleGeneration == capturedLifecycleGeneration else {
            await readSaved(force: true)
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
        activeSavedReadID = nil
        activeOperationLifecycleGeneration = nil
        isRefreshingSaved = false
        progress = nil
        failure = nil
    }
}
