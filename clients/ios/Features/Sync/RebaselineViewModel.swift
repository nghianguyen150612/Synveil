import Foundation
import Observation

@Observable
@MainActor
final class RebaselineViewModel {
    let library: Library
    private let coordinator: RebaselineCoordinator?
    private let sessionController: SessionController
    private let statusDidChange: @MainActor () async -> Void
    private let revision: UInt64
    private var scope: ClientMutationScope?
    private var task: Task<Void, Never>?
    private(set) var progress: RebaselineProgress?
    private(set) var failure: RebaselineFailure?
    private(set) var isBusy = false
    private(set) var invalidated = false
    init(
        library: Library, coordinator: RebaselineCoordinator?, sessionController: SessionController,
        statusDidChange: @escaping @MainActor () async -> Void = {}
    ) {
        self.library = library
        self.coordinator = coordinator
        self.sessionController = sessionController
        self.statusDidChange = statusDidChange
        revision = sessionController.lifecycleRevision
    }
    var isCurrent: Bool {
        !invalidated && sessionController.state == .authenticated
            && revision == sessionController.lifecycleRevision
    }
    var canStart: Bool {
        isCurrent && !isBusy && coordinator != nil && scope != nil && library.status != .quarantined
            && (progress == nil
                || [.startUnknown, .expired, .blocked, .activeComplete].contains(progress!.state))
    }
    var action: RebaselineRecoveryAction? { isCurrent && !isBusy ? progress?.recoveryAction : nil }
    func loadStatus() async {
        guard isCurrent, !isBusy else {
            sessionDidChange()
            return
        }
        guard let coordinator else {
            failure = .storage(.unavailable)
            return
        }
        do {
            let captured = try await coordinator.scope(libraryId: library.id)
            let saved = try await coordinator.status(scope: captured)
            guard isCurrent else {
                sessionDidChange()
                return
            }
            scope = captured
            progress = saved
        } catch {
            guard isCurrent else {
                sessionDidChange()
                return
            }
            failure = RebaselineFailure.classify(error)
        }
    }
    func start(confirmedByUser: Bool) async {
        guard confirmedByUser, canStart, let coordinator, let scope else { return }
        await execute {
            try await coordinator.start(scope: scope, library: self.library, confirmedByUser: true)
        }
    }
    func continueSnapshot(confirmedByUser: Bool = false) async {
        guard let action, let coordinator, let scope else { return }
        switch action {
        case .resumeDownload:
            await execute {
                try await coordinator.download(scope: scope) { [weak self] in
                    if self?.isCurrent == true { self?.progress = $0 }
                }
            }
        case .prepare: await execute { try await coordinator.prepare(scope: scope) }
        case .complete, .recoverOriginalCompletion, .activateConfirmed:
            guard confirmedByUser else { return }
            await execute {
                try await coordinator.complete(
                    scope: scope, recovering: action != .complete, confirmedByUser: true)
            }
        case .repeatStart: await start(confirmedByUser: confirmedByUser)
        }
    }
    private func execute(_ operation: @escaping @MainActor () async throws -> Void) async {
        guard isCurrent, task == nil else { return }
        isBusy = true
        failure = nil
        let work = Task { @MainActor in
            do { try await operation() } catch {
                if self.isCurrent { self.failure = RebaselineFailure.classify(error) }
            }
        }
        task = work
        await withTaskCancellationHandler {
            await work.value
        } onCancel: {
            work.cancel()
        }
        task = nil
        isBusy = false
        guard isCurrent else {
            sessionDidChange()
            return
        }
        // The read happens outside the cancelled operation and derives counters from committed rows.
        await loadStatus()
        guard isCurrent else { return }
        await statusDidChange()
    }
    func cancel() { task?.cancel() }
    func sessionDidChange() {
        guard !isCurrent else { return }
        invalidated = true
        task?.cancel()
        scope = nil
        progress = nil
        failure = nil
    }
    var message: String {
        guard isCurrent else { return "The session changed. Return to the Library catalog." }
        if let failure {
            switch failure {
            case .incompatibleMutations:
                return
                    "Pending or uncertain changes need reconciliation before rebuilding. Their original requests are preserved."
            case .incompatibleSync, .alreadyRunning:
                return "Another synchronization or acknowledgment needs to finish first."
            case .capacity:
                return
                    "The snapshot exceeds available local storage. The previous saved metadata is preserved."
            case .expired:
                return progress?.state == .reconciliationRequired
                    ? "Completion evidence is no longer retained by the server. Authoritative reconciliation is required; the prepared snapshot and previous metadata are preserved."
                    : "The server reports that this bootstrap expired. Start a new snapshot explicitly."
            case .cancelled: return "Cancelled. Committed snapshot progress remains saved."
            default:
                return
                    "The operation could not be verified. Saved evidence and previous metadata are preserved."
            }
        }
        guard let progress else {
            return
                "Rebuild saved folder structure from a complete server snapshot. Your current saved metadata remains available during downloading. File contents are not downloaded."
        }
        switch progress.state {
        case .startUnknown:
            return
                "Snapshot request status unknown. Confirm another Start to retrieve the server's existing OPEN bootstrap."
        case .bootstrapOpen, .downloading:
            return "Downloading saved metadata. Continue to read up to 16 pages in the foreground."
        case .terminalReceived:
            return
                "Terminal manifest received. Verify the complete folder graph and prepare replacement metadata."
        case .prepared:
            return
                "Replacement metadata is durably prepared. Confirm the server checkpoint handoff before applying it."
        case .completionInFlight, .outcomeUnknown:
            return
                "Completion status unknown. The server may have accepted the original token. Confirm recovery to check it once."
        case .completionConfirmed:
            return
                "Server completion is confirmed. Explicit recovery applies the prepared snapshot locally."
        case .activeComplete:
            return
                "Complete saved folder structure at snapshot sequence \(progress.position?.sequence.rawValue ?? ""). Newer server changes may exist. File contents and live timestamps are not downloaded."
        case .expired: return "Bootstrap expired. A new generation requires an explicit Start."
        case .blocked, .reconciliationRequired:
            return "Recovery requires authoritative reconciliation. Prepared evidence is preserved."
        }
    }
}
