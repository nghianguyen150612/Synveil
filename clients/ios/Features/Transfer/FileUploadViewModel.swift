import Foundation
import Observation

@Observable
@MainActor
final class FileUploadViewModel {
    private(set) var backgroundJobs: [UploadOperationId: BackgroundUploadJob] = [:]
    private(set) var records: [UploadRecord] = []
    private(set) var liveProgress: UploadProgress?
    private(set) var failure: UploadFailure?
    private(set) var isRunning = false
    private(set) var isPreparingSource = false
    private(set) var isLoadingSaved = false
    private(set) var activityMessage: String?
    private(set) var savedStatusUnavailable = false

    private let coordinator: FileUploadCoordinator?
    private let activityReader: (any FileUploadActivityReaderProtocol)?
    private let controller: SessionController
    private let library: LibraryId
    private let libraryStatus: LibraryStatus
    private let sessionRevision: UInt64
    @ObservationIgnored private var invalidated = false
    @ObservationIgnored private var statusGeneration: UInt64 = 0

    init(
        coordinator: FileUploadCoordinator?, controller: SessionController,
        library: LibraryId, libraryStatus: LibraryStatus,
        activityReader: (any FileUploadActivityReaderProtocol)? = nil
    ) {
        self.coordinator = coordinator
        self.activityReader = activityReader ?? coordinator
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

    func canResume(_ record: UploadRecord) -> Bool {
        isAvailable && !isRunning && !isLoadingSaved && !savedStatusUnavailable
            && record.state.canResume && backgroundJobs[record.id]?.blocksForeground != true
    }

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
        await readSaved(visibleOnly: false)
    }

    private func readSaved(visibleOnly: Bool) async {
        guard isCurrent, !isLoadingSaved, !isRunning, let activityReader else { return }
        statusGeneration &+= 1
        let generation = statusGeneration
        isLoadingSaved = true
        defer { if generation == statusGeneration { isLoadingSaved = false } }
        do {
            let saved: [UploadRecord]
            if visibleOnly {
                saved = try await activityReader.visibleSaved(library: library)
            } else {
                saved = try await activityReader.saved(library: library, limit: 20)
            }
            guard isCurrent, !isRunning, generation == statusGeneration else { return }
            var jobs: [UploadOperationId: BackgroundUploadJob] = [:]
            for record in saved {
                if let job = try await activityReader.backgroundJob(record.id) {
                    jobs[record.id] = job
                }
                guard isCurrent, !isRunning, generation == statusGeneration else { return }
            }
            records = saved
            backgroundJobs = jobs
            savedStatusUnavailable = false
            if visibleOnly { liveProgress = nil }
            failure = nil
        } catch {
            guard isCurrent, generation == statusGeneration else { return }
            savedStatusUnavailable = true
            failure = UploadPolicy.classify(error)
        }
    }

    func refreshBackgroundState() async {
        await readSaved(visibleOnly: true)
    }

    func start(selectedURL: URL, target: UploadTarget, backgroundEnabled: Bool = false) async
        -> FileUploadResult
    {
        guard isAvailable, !isRunning, !isLoadingSaved, let coordinator else {
            return .stopped(nil, .invalidTarget)
        }
        return await runTransfer(activity: "Preparing file and calculating SHA-256…") {
            await coordinator.start(
                selectedURL: selectedURL, target: target, library: library,
                libraryStatus: libraryStatus, backgroundEnabled: backgroundEnabled, progress: $0)
        }
    }

    func resume(_ record: UploadRecord, backgroundEnabled: Bool = false) async -> FileUploadResult {
        guard canResume(record), let coordinator else {
            return .stopped(record, .invalidTarget)
        }
        return await runTransfer(activity: "Checking saved source and server status…") {
            await coordinator.resume(
                id: record.id, library: library, libraryStatus: libraryStatus,
                backgroundEnabled: backgroundEnabled, progress: $0)
        }
    }

    func checkServerStatus(_ record: UploadRecord) async -> Result<UploadRecord, UploadFailure> {
        guard canInspectSaved, !isRunning, !isLoadingSaved, !savedStatusUnavailable,
            let coordinator, record.serverSessionId != nil
        else {
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
            guard isCurrent else { return .failure(.staleSession) }
            if updated.state == .committed { liveProgress = UploadProgress(updated) }
        case .failure(let problem): failure = problem
        }
        return result
    }

    func abort(_ record: UploadRecord) async -> Result<UploadRecord, UploadFailure> {
        guard canInspectSaved, !isRunning, !isLoadingSaved, !savedStatusUnavailable,
            let coordinator, record.serverSessionId != nil
        else {
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
            guard isCurrent else { return .failure(.staleSession) }
            if updated.state == .committed { liveProgress = UploadProgress(updated) }
        case .failure(let problem): failure = problem
        }
        return result
    }

    func pause() async {
        guard isCurrent else { return }
        do { try await coordinator?.pauseUserTransfer() } catch {
            guard isCurrent else { return }
            failure = UploadPolicy.classify(error)
        }
    }

    func pauseBackground(_ record: UploadRecord) async {
        guard isCurrent, let background = coordinator?.background else { return }
        do { try await background.pause(record.id) } catch {
            guard isCurrent else { return }
            failure = UploadPolicy.classify(error)
        }
        await refreshBackgroundState()
    }

    func suspendForeground() { coordinator?.cancel() }

    func reportImportFailure() { if isCurrent { failure = .sourceUnavailable } }

    func invalidate() {
        guard !invalidated else { return }
        invalidated = true
        statusGeneration &+= 1
        isLoadingSaved = false
        coordinator?.cancel()
        records = []
        backgroundJobs = [:]
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
        case .completed(let record), .scheduled(let record):
            liveProgress = UploadProgress(record)
        case .stopped(let record, let problem):
            if let record { liveProgress = UploadProgress(record) }
            failure = problem
        }
        await refreshRecords()
        return result
    }

    private func refreshRecords() async {
        // Reload both durable operation state and background ownership before offering Resume.
        let operationFailure = failure
        await readSaved(visibleOnly: false)
        if isCurrent, !savedStatusUnavailable { failure = operationFailure }
    }
}
