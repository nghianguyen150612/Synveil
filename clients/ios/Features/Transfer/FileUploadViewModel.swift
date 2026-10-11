import Foundation
import Observation

@Observable
@MainActor
final class FileUploadViewModel {
    private(set) var records: [UploadRecord] = []
    private(set) var liveProgress: UploadProgress?
    private(set) var failure: UploadFailure?
    private(set) var isRunning = false
    private(set) var isPreparingSource = false
    private(set) var isLoadingSaved = false
    private(set) var activityMessage: String?

    private let coordinator: FileUploadCoordinator?
    private let controller: SessionController
    private let library: LibraryId
    private let libraryStatus: LibraryStatus
    private let sessionRevision: UInt64
    @ObservationIgnored private var invalidated = false

    init(
        coordinator: FileUploadCoordinator?, controller: SessionController,
        library: LibraryId, libraryStatus: LibraryStatus
    ) {
        self.coordinator = coordinator
        self.controller = controller
        self.library = library
        self.libraryStatus = libraryStatus
        sessionRevision = controller.lifecycleRevision
    }

    var isCurrent: Bool {
        !invalidated && controller.state == .authenticated
            && controller.lifecycleRevision == sessionRevision
    }

    var isAvailable: Bool { isCurrent && coordinator != nil && libraryStatus == .active }
    var canInspectSaved: Bool { isCurrent && coordinator != nil }

    var unavailableMessage: String {
        guard coordinator != nil else {
            return "Private upload storage is unavailable. Reopen Synveil after storage recovery."
        }
        switch libraryStatus {
        case .active: return "Uploads are available for this Library."
        case .readOnly: return "This Library is read-only. Uploads are disabled."
        case .quarantined: return "This Library is quarantined. Uploads are disabled."
        }
    }

    func displayedProgress(for record: UploadRecord) -> UploadProgress {
        if let liveProgress, liveProgress.operationId == record.id { return liveProgress }
        return UploadProgress(record)
    }

    func loadSaved() async {
        guard isCurrent, !isLoadingSaved, !isRunning, let coordinator else { return }
        isLoadingSaved = true
        defer { isLoadingSaved = false }
        do {
            let saved = try await coordinator.saved(library: library, limit: 20)
            guard isCurrent, !isRunning else { return }
            records = saved
            failure = nil
        } catch {
            guard isCurrent else { return }
            failure = UploadPolicy.classify(error)
        }
    }

    func start(selectedURL: URL, target: UploadTarget) async -> FileUploadResult {
        guard isAvailable, !isRunning, let coordinator else {
            return .stopped(nil, .invalidTarget)
        }
        return await runTransfer(activity: "Preparing file and calculating SHA-256…") {
            await coordinator.start(
                selectedURL: selectedURL, target: target, library: library,
                libraryStatus: libraryStatus, progress: $0)
        }
    }

    func resume(_ record: UploadRecord) async -> FileUploadResult {
        guard isAvailable, !isRunning, let coordinator, record.state.canResume else {
            return .stopped(record, .invalidTarget)
        }
        return await runTransfer(activity: "Checking saved source and server status…") {
            await coordinator.resume(
                id: record.id, library: library, libraryStatus: libraryStatus, progress: $0)
        }
    }

    func checkServerStatus(_ record: UploadRecord) async -> Result<UploadRecord, UploadFailure> {
        guard canInspectSaved, !isRunning, let coordinator, record.serverSessionId != nil else {
            return .failure(.invalidTarget)
        }
        isRunning = true
        activityMessage = "Checking Server Status…"
        failure = nil
        let result = await coordinator.checkServerStatus(
            id: record.id, library: library)
        isRunning = false
        activityMessage = nil
        guard isCurrent else { return .failure(.staleSession) }
        switch result {
        case .success(let updated):
            await refreshRecords()
            if updated.state == .committed { liveProgress = UploadProgress(updated) }
        case .failure(let problem): failure = problem
        }
        return result
    }

    func abort(_ record: UploadRecord) async -> Result<UploadRecord, UploadFailure> {
        guard canInspectSaved, !isRunning, let coordinator, record.serverSessionId != nil else {
            return .failure(.invalidTarget)
        }
        isRunning = true
        activityMessage = "Requesting server abort…"
        failure = nil
        let result = await coordinator.abort(
            id: record.id, library: library)
        isRunning = false
        activityMessage = nil
        guard isCurrent else { return .failure(.staleSession) }
        switch result {
        case .success(let updated):
            await refreshRecords()
            if updated.state == .committed { liveProgress = UploadProgress(updated) }
        case .failure(let problem): failure = problem
        }
        return result
    }

    func pause() { coordinator?.cancel() }

    func reportImportFailure() { if isCurrent { failure = .sourceUnavailable } }

    func invalidate() {
        guard !invalidated else { return }
        invalidated = true
        coordinator?.cancel()
        records = []
        liveProgress = nil
        failure = nil
        activityMessage = nil
    }

    private func runTransfer(
        activity: String,
        _ operation:
            @MainActor (@escaping @MainActor (UploadProgress) -> Void) async
            -> FileUploadResult
    ) async -> FileUploadResult {
        isRunning = true
        isPreparingSource = true
        activityMessage = activity
        liveProgress = nil
        failure = nil
        let result = await operation { [weak self] progress in
            guard let self, self.isCurrent else { return }
            self.isPreparingSource = false
            self.activityMessage = nil
            self.liveProgress = progress
        }
        isRunning = false
        isPreparingSource = false
        activityMessage = nil
        guard isCurrent else {
            records = []
            liveProgress = nil
            return result
        }
        switch result {
        case .completed(let record):
            liveProgress = UploadProgress(record)
        case .stopped(let record, let problem):
            if let record { liveProgress = UploadProgress(record) }
            failure = problem
        }
        await refreshRecords()
        return result
    }

    private func refreshRecords() async {
        guard isCurrent, !isRunning, let coordinator else { return }
        do {
            records = try await coordinator.saved(library: library, limit: 20)
        } catch {
            failure = UploadPolicy.classify(error)
        }
    }
}
