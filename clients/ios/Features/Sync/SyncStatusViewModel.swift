import Foundation
import Observation

enum SyncStatusUIState: Equatable {
    case idle, checkingLocalStatus, checkpointRequired, ready, syncing, upToDate, moreWork
    case pendingAck, unknownAck, reconciliationRequired, offline, failed, cancelled, invalidated
}

@Observable
@MainActor
final class SyncStatusViewModel {
    let library: Library
    private(set) var state: SyncStatusUIState = .idle
    private(set) var progress = InboundSyncProgress.initial
    private(set) var completeness: NodeProjectionCompleteness?
    private(set) var recovery: SyncRecoveryAction?
    private(set) var pendingState: InboundSyncPageState?
    private(set) var stopReason: InboundSyncStopReason?
    @ObservationIgnored private let coordinator: (any InboundSyncCoordinatorProtocol)?
    @ObservationIgnored private let projection: (any NodeProjectionRepositoryProtocol)?
    @ObservationIgnored private let checkpoint: (any SyncCheckpointPreparationProtocol)?
    @ObservationIgnored private let sessionController: SessionController
    @ObservationIgnored private let sessionRevision: UInt64
    @ObservationIgnored private var scope: ClientMutationScope?
    @ObservationIgnored private var task: Task<InboundSyncRunResult, Never>?
    @ObservationIgnored private var invalidated = false

    init(
        library: Library, coordinator: (any InboundSyncCoordinatorProtocol)?,
        projection: (any NodeProjectionRepositoryProtocol)?,
        checkpoint: (any SyncCheckpointPreparationProtocol)?, sessionController: SessionController
    ) {
        self.library = library
        self.coordinator = coordinator
        self.projection = projection
        self.checkpoint = checkpoint
        self.sessionController = sessionController
        sessionRevision = sessionController.lifecycleRevision
    }

    private var isCurrent: Bool {
        !invalidated && sessionController.state == .authenticated
            && sessionController.lifecycleRevision == sessionRevision
    }
    var isBusy: Bool { state == .syncing || state == .checkingLocalStatus }
    var canSync: Bool {
        guard isCurrent, scope != nil, coordinator != nil, !isBusy,
            library.status != .quarantined
        else { return false }
        return [.ready, .pendingAck, .upToDate, .moreWork, .cancelled].contains(state)
            || state == .failed && stopReason == .serverUnavailable
    }
    var canSetup: Bool { isCurrent && !isBusy && state == .checkpointRequired && checkpoint != nil }
    var canRecover: Bool {
        isCurrent && !isBusy && state == .unknownAck && recovery?.canRecover == true
            && library.status != .quarantined
    }

    /// Local-only inspection. Reopening this screen never authorizes network work.
    func loadStatus() async {
        guard isCurrent, !isBusy else {
            sessionDidChange()
            return
        }
        guard library.status != .quarantined else {
            finish(.scopeMismatch)
            return
        }
        guard let coordinator, let projection else {
            finish(.storageFailure)
            return
        }
        state = .checkingLocalStatus
        do {
            if scope == nil { scope = try await coordinator.scope(libraryId: library.id) }
            guard isCurrent, let scope else {
                sessionDidChange()
                return
            }
            let status = await coordinator.status(scope: scope)
            guard isCurrent else {
                sessionDidChange()
                return
            }
            // Repository is independently injectable and reads the same persisted projection.
            // A failed local read cannot be masked by an in-memory success flag.
            let cached = await projection.projectionState(scope: scope)
            guard isCurrent else {
                sessionDidChange()
                return
            }
            if case .loaded(let value) = cached {
                completeness = value.completeness
            } else if status.stopReason != .checkpointRequired {
                if case .unavailable(let failure) = cached {
                    finish(InboundSyncCoordinator.classify(failure))
                    return
                }
            }
            progress = status.progress
            completeness = status.completeness
            recovery = status.recovery
            pendingState = status.pendingState
            if let reason = status.stopReason {
                finish(reason)
            } else {
                stopReason = nil
                state = status.pendingState == .appliedAckPending ? .pendingAck : .ready
            }
        } catch {
            guard isCurrent else {
                sessionDidChange()
                return
            }
            finish(InboundSyncCoordinator.classify(error))
        }
    }

    func syncNow() async {
        guard canSync, let coordinator, let scope else { return }
        await execute {
            await coordinator.synchronize(scope: scope, configuration: .foreground) { [weak self] in
                self?.publish($0)
            }
        }
    }

    /// Explicit offline retry is a user action; no reachability callback schedules work.
    func retryWhenConnectionAvailable() async {
        guard isCurrent, state == .offline, coordinator != nil, scope != nil else { return }
        state = .ready
        await syncNow()
    }

    func recoverAcknowledgement(confirmedByUser: Bool) async {
        guard confirmedByUser, canRecover, let coordinator, let scope, let recovery else { return }
        await execute {
            await coordinator.recoverUnknownAcknowledgement(
                scope: scope, position: recovery.position, confirmedByUser: true
            ) { [weak self] in self?.publish($0) }
        }
    }

    func prepareCheckpoint(confirmedByUser: Bool) async {
        guard confirmedByUser, canSetup, let checkpoint, let scope else { return }
        state = .checkingLocalStatus
        let result = await checkpoint.prepare(scope: scope)
        guard isCurrent else {
            sessionDidChange()
            return
        }
        switch result {
        case .prepared:
            state = .idle
            await loadStatus()
        case .failed(let failure): finish(InboundSyncCoordinator.classify(failure))
        }
    }

    private func execute(_ operation: @escaping @MainActor () async -> InboundSyncRunResult) async {
        guard task == nil, isCurrent else { return }
        state = .syncing
        progress = .initial
        let operationTask = Task { await operation() }
        task = operationTask
        let result = await withTaskCancellationHandler {
            await operationTask.value
        } onCancel: {
            operationTask.cancel()
        }
        task = nil
        guard isCurrent else {
            sessionDidChange()
            return
        }
        progress = result.progress
        recovery = result.recovery
        finish(result.reason)
        guard isCurrent else { return }
        if result.reason == .awaitingAckRecovery, recovery == nil,
            let coordinator, let scope
        {
            let saved = await coordinator.status(scope: scope)
            guard isCurrent else {
                sessionDidChange()
                return
            }
            recovery = saved.recovery
            if let reason = saved.stopReason { finish(reason) }
        }
        // Read local completeness only. Final observed empty-feed / budget outcome remains intact.
        if let projection, let scope {
            let cached = await projection.projectionState(scope: scope)
            guard isCurrent else {
                sessionDidChange()
                return
            }
            if case .loaded(let value) = cached { completeness = value.completeness }
        }
    }

    private func publish(_ value: InboundSyncProgress) {
        guard isCurrent, state == .syncing else { return }
        progress = value
    }
    func cancel() { task?.cancel() }
    func sessionDidChange() {
        guard !isCurrent else { return }
        invalidate()
    }
    private func invalidate() {
        invalidated = true
        task?.cancel()
        scope = nil
        progress = .initial
        completeness = nil
        recovery = nil
        pendingState = nil
        stopReason = nil
        state = .invalidated
    }
    private func finish(_ reason: InboundSyncStopReason) {
        stopReason = reason
        switch reason {
        case .upToDate: state = .upToDate
        case .progressed: state = .ready
        case .moreWork: state = .moreWork
        case .checkpointRequired: state = .checkpointRequired
        case .awaitingAckRecovery, .recoveryLimitReached: state = .unknownAck
        case .reconciliationRequired, .rebaselineRequired, .metadataChanged,
            .missingMaterialization:
            state = .reconciliationRequired
        case .offline: state = .offline
        case .cancelled: state = .cancelled
        case .committedButSessionChanged: invalidate()
        default: state = .failed
        }
    }

    var message: String {
        if state == .invalidated {
            return
                "The session changed. Return to the Library catalog to inspect the current authenticated scope. Previously committed work may remain saved."
        }
        if state == .syncing { return Self.phaseLabel(progress.phase) }
        if state == .checkingLocalStatus { return "Checking saved synchronization status…" }
        if state == .pendingAck {
            return "Metadata is applied locally. Sync Now will acknowledge it."
        }
        if state == .ready, pendingState == .receivedUnapplied {
            return
                "Saved inbound changes are waiting to be applied. Sync Now resumes this page before fetching more changes."
        }
        guard let reason = stopReason else {
            return
                "Sync Now reads inbound metadata. Send Pending Changes remains a separate action."
        }
        switch reason {
        case .upToDate: return "No new changes."
        case .progressed:
            return "Acknowledgment confirmed. Choose Sync Now to check for more changes."
        case .moreWork: return "More changes may be available. Sync again to continue."
        case .checkpointRequired:
            return
                "Set up the verified synchronization checkpoint first. Setup may initialize synchronization state on the server."
        case .awaitingAckRecovery, .recoveryLimitReached:
            return
                "The server may have received the previous acknowledgment. Its status has not been confirmed."
        case .metadataChanged:
            return
                "Server changed while metadata was being read. Synchronization reconciliation is required."
        case .missingMaterialization:
            return
                "Required Node metadata is unavailable. Synchronization reconciliation is required."
        case .reconciliationRequired:
            return
                "Local journal position does not match the server checkpoint. Review synchronization recovery before continuing."
        case .rebaselineRequired:
            return
                "Synchronization recovery is required. Existing metadata and pending changes are preserved. Full rebaseline is not available yet."
        case .authenticationRequired:
            return "Authentication needs recovery. Return to the session recovery screen."
        case .deviceRevoked:
            return "This Device has been revoked. Restore authorized access before synchronizing."
        case .offline:
            return "Offline. Saved metadata and progress are retained. Try again when connected."
        case .serverUnavailable:
            return "A temporary network or server error occurred. Saved progress is retained."
        case .storageFailure:
            return "Local storage is unavailable. Live browsing remains available."
        case .protocolFailure, .invalidConfiguration:
            return
                "The synchronization request or response could not be verified. Progress has not been assumed."
        case .cancelled:
            return
                "Synchronization cancelled. Staged or committed work may remain saved. Sync Now checks it before continuing."
        case .committedButSessionChanged:
            return "The session changed. Committed work may remain saved in its original scope."
        case .scopeMismatch: return "This Library does not match the active authenticated scope."
        case .alreadyRunning:
            return "Another synchronization operation is running. Wait for it to finish."
        case .recoveryConfirmationRequired:
            return "Confirm recovery before retrying the original acknowledgment."
        }
    }

    var cacheMessage: String {
        switch completeness {
        case .complete: return "Complete metadata projection. File contents are not downloaded."
        case .partial: return "Partial metadata cache. Some folders and items may be missing."
        case .uninitialized, nil: return "No complete Library snapshot has been initialized."
        case .rebaselineRequired: return "Saved metadata requires synchronization recovery."
        }
    }
    static func phaseLabel(_ phase: InboundSyncPhase) -> String {
        switch phase {
        case .checkingLocalStatus: "Checking saved status"
        case .fetching: "Reading changes"
        case .staged: "Changes staged locally"
        case .applying: "Reading and applying Node metadata"
        case .applied: "Metadata committed locally"
        case .acknowledging: "Confirming acknowledgment"
        case .confirmed: "Server checkpoint confirmed"
        case .stopped: "Synchronization stopped"
        }
    }
}
