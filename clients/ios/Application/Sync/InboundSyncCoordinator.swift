import Foundation

/// Explicit foreground orchestration. No work starts from init, status reads or session recovery.
/// The database owns global run exclusion and P039 owns all application/ACK transitions.
@MainActor
final class InboundSyncCoordinator: InboundSyncCoordinatorProtocol {
    private let feed: SyncFeedService
    private let application: SyncFeedApplicationService
    private let ack: SyncAckService
    private let projection: any NodeProjectionRepositoryProtocol
    private let queue: DurableMutationQueue
    private let database: MutationQueueSQLiteStore
    private let provider: any AuthenticatedSyncFeedRequestProviderProtocol
    private let bridge: any RustBridgeProtocol

    init(
        feed: SyncFeedService, application: SyncFeedApplicationService, ack: SyncAckService,
        projection: any NodeProjectionRepositoryProtocol, queue: DurableMutationQueue,
        database: MutationQueueSQLiteStore,
        provider: any AuthenticatedSyncFeedRequestProviderProtocol,
        bridge: any RustBridgeProtocol
    ) {
        self.feed = feed
        self.application = application
        self.ack = ack
        self.projection = projection
        self.queue = queue
        self.database = database
        self.provider = provider
        self.bridge = bridge
    }

    func scope(libraryId: LibraryId) async throws -> ClientMutationScope {
        let session = try await provider.begin()
        let scope = try await session.mutationScope(libraryId: libraryId, bridge: bridge)
        try await provider.validate(session)
        return scope
    }

    private struct Inspection {
        let status: InboundSyncStatus
        let page: InboundSyncPageRecord?
        let base: ClientMutationBase?
    }

    private func inspect(scope: ClientMutationScope, session: LibraryRequestScope) async throws
        -> Inspection
    {
        try await queue.validateExecutionSession(session)
        let page = try await database.oldestUnresolvedInbound(
            scope: scope, credentialId: session.credentialIdentifier, bridge: bridge)
        try await queue.validateExecutionSession(session)
        let state: NodeProjectionState
        switch await projection.projectionState(scope: scope) {
        case .loaded(let value): state = value
        case .unavailable(.storage(.syncBaseUnavailable)):
            try await queue.validateExecutionSession(session)
            return Inspection(
                status: InboundSyncStatus(
                    progress: .initial, completeness: .uninitialized, pendingState: page?.state,
                    recovery: nil, stopReason: .checkpointRequired), page: page, base: nil)
        case .unavailable(let failure): throw failure
        }
        try await queue.validateExecutionSession(session)
        let recovery: SyncRecoveryAction?
        if let page, page.state == .ackInFlight {
            let count = try await database.inboundAckAttemptCount(
                scope: scope, position: page.page.start, credentialId: session.credentialIdentifier)
            recovery = SyncRecoveryAction(position: page.page.start, attempts: count)
        } else {
            recovery = nil
        }
        let confirmed = try await database.latestConfirmedInbound(
            scope: scope, credentialId: session.credentialIdentifier, bridge: bridge)
        try await queue.validateExecutionSession(session)
        let observed = page?.page ?? confirmed?.page
        let progress = InboundSyncProgress(
            phase: .checkingLocalStatus, pagesProcessed: 0, eventsApplied: 0,
            locallyApplied: state.locallyApplied, serverConfirmed: state.serverConfirmed,
            observedHighWatermark: observed.map {
                SyncJournalPosition(epoch: $0.start.epoch, sequence: $0.highWatermark)
            })
        var reason: InboundSyncStopReason?
        let base: ClientMutationBase?
        do { base = try await queue.persistedBase(scope: scope) } catch {
            reason = Self.classify(error)
            base = nil
        }
        try await queue.validateExecutionSession(session)
        if state.completeness == .rebaselineRequired || page?.state == .blockedRebaseline {
            reason = .rebaselineRequired
        }
        if let base {
            let position = SyncJournalPosition(epoch: base.epoch, sequence: base.sequence)
            if state.serverConfirmed != position { reason = .reconciliationRequired }
            if let confirmed,
                confirmed.page.start.epoch != base.epoch
                    || SyncDecimalValidation.less(
                        base.sequence.rawValue, confirmed.page.through.rawValue)
                    || state.locallyApplied == nil
            {
                reason = .reconciliationRequired
            }
            if let page {
                if page.page.scope != scope || page.page.start != position {
                    reason = .reconciliationRequired
                } else if page.state == .receivedUnapplied {
                    if let applied = state.locallyApplied, applied != position {
                        reason = .reconciliationRequired
                    }
                } else if [.appliedAckPending, .ackInFlight].contains(page.state) {
                    let through = SyncJournalPosition(
                        epoch: base.epoch, sequence: page.page.through)
                    if state.locallyApplied != through { reason = .reconciliationRequired }
                }
            } else if let applied = state.locallyApplied, applied != position {
                reason = .reconciliationRequired
            }
        }
        if reason == nil, let recovery {
            reason = recovery.canRecover ? .awaitingAckRecovery : .recoveryLimitReached
        }
        return Inspection(
            status: InboundSyncStatus(
                progress: progress, completeness: state.completeness, pendingState: page?.state,
                recovery: recovery, stopReason: reason), page: page, base: base)
    }

    func status(scope: ClientMutationScope) async -> InboundSyncStatus {
        do {
            let session = try await queue.capture(scope: scope)
            return try await inspect(scope: scope, session: session).status
        } catch {
            return InboundSyncStatus(
                progress: .initial, completeness: nil, pendingState: nil, recovery: nil,
                stopReason: Self.classify(error))
        }
    }

    func synchronize(
        scope: ClientMutationScope, configuration: InboundSyncRunConfiguration = .foreground,
        progress: @MainActor (InboundSyncProgress) -> Void = { _ in }
    ) async -> InboundSyncRunResult {
        guard configuration.isValid else { return Self.result(.invalidConfiguration) }
        let owner = UUID()
        do {
            let session = try await queue.capture(scope: scope)
            try await database.claimInboundRun(
                scope: scope, credentialId: session.credentialIdentifier, owner: owner)
            let result = await run(
                scope: scope, session: session, configuration: configuration, publish: progress)
            await database.finishInboundRun(owner: owner)
            return result
        } catch { return Self.result(Self.classify(error)) }
    }

    private func run(
        scope: ClientMutationScope, session: LibraryRequestScope,
        configuration: InboundSyncRunConfiguration,
        publish: @MainActor (InboundSyncProgress) -> Void
    ) async -> InboundSyncRunResult {
        var current = InboundSyncProgress.initial
        var recovery: SyncRecoveryAction?
        do {
            // Finite loop includes resumed pages. A staged page larger than the remaining event
            // budget stays durable and is deferred; a new GET always uses the remaining budget.
            for _ in 0..<configuration.maximumPages {
                try await queue.validateExecutionSession(session)
                let inspection = try await inspect(scope: scope, session: session)
                current = Self.snapshot(
                    inspection.status.progress, phase: .checkingLocalStatus,
                    pages: current.pagesProcessed, events: current.eventsApplied)
                publish(current)
                recovery = inspection.status.recovery
                if let reason = inspection.status.stopReason {
                    return Self.result(reason, current, recovery)
                }
                guard current.eventsApplied < configuration.maximumEvents else {
                    return Self.result(.moreWork, current)
                }
                let page: InboundSyncPageRecord
                if let pending = inspection.page {
                    page = pending
                } else {
                    current = Self.snapshot(current, phase: .fetching)
                    publish(current)
                    try await queue.validateExecutionSession(session)
                    let response = await feed.readAndStage(
                        scope: scope,
                        limit: min(
                            configuration.pageSize,
                            configuration.maximumEvents - current.eventsApplied))
                    if case .failed(let failure) = response {
                        return Self.result(Self.classify(failure), current)
                    }
                    try await queue.validateExecutionSession(session)
                    switch response {
                    case .noNewChanges(let position):
                        guard let base = inspection.base,
                            position
                                == SyncJournalPosition(epoch: base.epoch, sequence: base.sequence)
                        else { return Self.result(.reconciliationRequired, current) }
                        // An empty response proves an observation, never a complete local snapshot.
                        current = InboundSyncProgress(
                            phase: .confirmed, pagesProcessed: current.pagesProcessed,
                            eventsApplied: current.eventsApplied,
                            locallyApplied: current.locallyApplied,
                            serverConfirmed: current.serverConfirmed,
                            observedHighWatermark: position)
                        return Self.result(.upToDate, current)
                    case .staged(let record, _): page = record
                    case .failed(let failure): return Self.result(Self.classify(failure), current)
                    }
                }
                guard page.state == .receivedUnapplied || page.state == .appliedAckPending,
                    let base = inspection.base,
                    page.page.start
                        == SyncJournalPosition(epoch: base.epoch, sequence: base.sequence)
                else { return Self.result(.reconciliationRequired, current) }
                current = InboundSyncProgress(
                    phase: .staged, pagesProcessed: current.pagesProcessed,
                    eventsApplied: current.eventsApplied, locallyApplied: current.locallyApplied,
                    serverConfirmed: current.serverConfirmed,
                    observedHighWatermark: SyncJournalPosition(
                        epoch: page.page.start.epoch, sequence: page.page.highWatermark))
                publish(current)
                if page.state == .receivedUnapplied {
                    guard
                        page.page.events.count <= configuration.maximumEvents
                            - current.eventsApplied
                    else { return Self.result(.moreWork, current) }
                    current = Self.snapshot(current, phase: .applying)
                    publish(current)
                    try await queue.validateExecutionSession(session)
                    let result = await application.apply(scope: scope, position: page.page.start)
                    // Application already distinguishes committed work that cannot be published.
                    if case .committedButSessionChanged = result {
                        return Self.result(.committedButSessionChanged, current)
                    }
                    if case .failed(let failure) = result {
                        return Self.result(Self.classify(failure), current)
                    }
                    try await queue.validateExecutionSession(session)
                    switch result {
                    case .applied(let record, let state, let existing):
                        guard record.state == .appliedAckPending,
                            state.locallyApplied
                                == SyncJournalPosition(
                                    epoch: page.page.start.epoch, sequence: page.page.through)
                        else { return Self.result(.reconciliationRequired, current) }
                        current = InboundSyncProgress(
                            phase: .applied, pagesProcessed: current.pagesProcessed,
                            eventsApplied: current.eventsApplied
                                + (existing ? 0 : page.page.events.count),
                            locallyApplied: state.locallyApplied,
                            serverConfirmed: state.serverConfirmed,
                            observedHighWatermark: current.observedHighWatermark)
                        publish(current)
                    case .failed(let failure): return Self.result(Self.classify(failure), current)
                    case .committedButSessionChanged:
                        return Self.result(.committedButSessionChanged, current)
                    }
                }
                current = Self.snapshot(current, phase: .acknowledging)
                publish(current)
                let ackResult = try await acknowledge(
                    scope: scope, position: page.page.start, session: session, recovery: false)
                if Task.isCancelled, case .outcomeUnknown = ackResult {
                    return Self.result(.awaitingAckRecovery, current)
                }
                if case .failed(let failure) = ackResult,
                    [.authenticationRequired, .deviceRevoked].contains(Self.classify(failure))
                {
                    return Self.result(Self.classify(failure), current)
                }
                let refreshed = try await inspect(scope: scope, session: session)
                current = Self.snapshot(
                    refreshed.status.progress, phase: .acknowledging,
                    pages: current.pagesProcessed, events: current.eventsApplied)
                recovery = refreshed.status.recovery
                switch ackResult {
                case .confirmed:
                    guard refreshed.status.pendingState != .ackInFlight,
                        refreshed.status.pendingState != .appliedAckPending,
                        current.serverConfirmed == current.locallyApplied
                    else { return Self.result(.reconciliationRequired, current, recovery) }
                    current = Self.snapshot(
                        current, phase: .confirmed, pages: current.pagesProcessed + 1)
                    publish(current)
                    if let reason = refreshed.status.stopReason {
                        return Self.result(reason, current, recovery)
                    }
                case .outcomeUnknown:
                    if case .outcomeUnknown(let failure) = ackResult,
                        failure == .checkpointConflict || failure == .checkpointAheadOfProjection
                    {
                        return Self.result(.reconciliationRequired, current, recovery)
                    }
                    // A lost local return may follow a successful confirmation COMMIT. Readback
                    // distinguishes that fact, but ambiguity never authorizes another GET here.
                    if refreshed.status.pendingState == nil,
                        current.serverConfirmed == current.locallyApplied
                    {
                        current = Self.snapshot(
                            current, phase: .confirmed, pages: current.pagesProcessed + 1)
                        publish(current)
                        return Self.result(.progressed, current)
                    }
                    return Self.result(
                        refreshed.status.stopReason ?? .awaitingAckRecovery, current, recovery)
                case .failed(let failure):
                    if failure == .checkpointConflict || failure == .checkpointAheadOfProjection {
                        return Self.result(.reconciliationRequired, current, recovery)
                    }
                    return Self.result(
                        refreshed.status.stopReason ?? Self.classify(failure), current, recovery)
                }
            }
            return Self.result(.moreWork, current)
        } catch {
            // Receipt acquisition may commit ACK_IN_FLIGHT and lose its return before dispatch.
            // Validated local readback can refine status, but never authorizes another request.
            if !Task.isCancelled,
                let saved = try? await inspect(scope: scope, session: session),
                let reason = saved.status.stopReason,
                [.awaitingAckRecovery, .recoveryLimitReached, .rebaselineRequired].contains(reason)
            {
                let snapshot = Self.snapshot(
                    saved.status.progress, phase: .stopped,
                    pages: current.pagesProcessed, events: current.eventsApplied)
                return Self.result(reason, snapshot, saved.status.recovery)
            }
            return Self.result(Self.classify(error), current, recovery)
        }
    }

    private func acknowledge(
        scope: ClientMutationScope, position: SyncJournalPosition, session: LibraryRequestScope,
        recovery: Bool
    ) async throws -> SyncAckSubmissionResult {
        try await queue.validateExecutionSession(session)
        let receipt: AppliedFeedCommitReceipt
        if recovery {
            receipt = try await ack.recoveryReceipt(scope: scope, position: position)
        } else {
            receipt = try await ack.receipt(scope: scope, position: position)
        }
        do {
            guard receipt.matches(session) else { throw MutationQueueFailure.staleSession }
            try await queue.validateExecutionSession(session)
        } catch {
            await ack.discardUndispatchedReceipt(receipt)
            throw error
        }
        return await ack.acknowledge(receipt)
    }

    func recoverUnknownAcknowledgement(
        scope: ClientMutationScope, position: SyncJournalPosition, confirmedByUser: Bool,
        progress: @MainActor (InboundSyncProgress) -> Void = { _ in }
    ) async -> InboundSyncRunResult {
        guard confirmedByUser else { return Self.result(.recoveryConfirmationRequired) }
        let owner = UUID()
        let result: InboundSyncRunResult
        do {
            let session = try await queue.capture(scope: scope)
            try await database.claimInboundRun(
                scope: scope, credentialId: session.credentialIdentifier, owner: owner)
            result = await recover(
                scope: scope, position: position, session: session, publish: progress)
            await database.finishInboundRun(owner: owner)
        } catch { result = Self.result(Self.classify(error)) }
        return result
    }

    private func recover(
        scope: ClientMutationScope, position: SyncJournalPosition, session: LibraryRequestScope,
        publish: @MainActor (InboundSyncProgress) -> Void
    ) async -> InboundSyncRunResult {
        var current = InboundSyncProgress.initial
        var action: SyncRecoveryAction?
        do {
            let inspection = try await inspect(scope: scope, session: session)
            current = inspection.status.progress
            action = inspection.status.recovery
            guard inspection.status.stopReason == .awaitingAckRecovery,
                let recovery = action
            else {
                return Self.result(
                    inspection.status.stopReason ?? .reconciliationRequired, current, action)
            }
            guard recovery.position == position else {
                return Self.result(.reconciliationRequired, current, action)
            }
            current = Self.snapshot(current, phase: .acknowledging)
            publish(current)
            let response = try await acknowledge(
                scope: scope, position: position, session: session, recovery: true)
            if Task.isCancelled, case .outcomeUnknown = response {
                return Self.result(.awaitingAckRecovery, current)
            }
            if case .failed(let failure) = response,
                [.authenticationRequired, .deviceRevoked].contains(Self.classify(failure))
            {
                return Self.result(Self.classify(failure), current, action)
            }
            let refreshed = try await inspect(scope: scope, session: session)
            current = refreshed.status.progress
            action = refreshed.status.recovery
            switch response {
            case .confirmed:
                current = Self.snapshot(current, phase: .confirmed, pages: 1)
                publish(current)
                return Self.result(refreshed.status.stopReason ?? .progressed, current, action)
            case .outcomeUnknown:
                return Self.result(refreshed.status.stopReason ?? .progressed, current, action)
            case .failed(let failure):
                return Self.result(
                    refreshed.status.stopReason ?? Self.classify(failure), current, action)
            }
        } catch {
            if !Task.isCancelled,
                let saved = try? await inspect(scope: scope, session: session),
                let reason = saved.status.stopReason
            {
                return Self.result(reason, saved.status.progress, saved.status.recovery)
            }
            return Self.result(Self.classify(error), current, action)
        }
    }

    private static func snapshot(
        _ value: InboundSyncProgress, phase: InboundSyncPhase, pages: Int? = nil, events: Int? = nil
    ) -> InboundSyncProgress {
        InboundSyncProgress(
            phase: phase, pagesProcessed: pages ?? value.pagesProcessed,
            eventsApplied: events ?? value.eventsApplied, locallyApplied: value.locallyApplied,
            serverConfirmed: value.serverConfirmed,
            observedHighWatermark: value.observedHighWatermark)
    }

    private static func result(
        _ reason: InboundSyncStopReason, _ progress: InboundSyncProgress = .initial,
        _ recovery: SyncRecoveryAction? = nil
    ) -> InboundSyncRunResult {
        InboundSyncRunResult(
            reason: reason, progress: snapshot(progress, phase: .stopped), recovery: recovery)
    }

    static func classify(_ error: any Error) -> InboundSyncStopReason {
        if let failure = error as? SyncFeedFailure {
            switch failure {
            case .scopeMismatch: return .scopeMismatch
            case .staleSession: return .committedButSessionChanged
            case .rebaselineRequired: return .rebaselineRequired
            case .checkpointConflict, .checkpointAheadOfProjection, .applicationCommitRequired:
                return .reconciliationRequired
            case .cancelled: return .cancelled
            case .protocolFailure: return .protocolFailure
            case .storage(let value): return classify(value)
            case .transport(let value): return classify(value)
            }
        }
        if let failure = error as? SyncProjectionFailure {
            switch failure {
            case .storage(let value): return classify(value)
            case .transport(let value): return classify(value)
            case .scopeMismatch: return .scopeMismatch
            case .staleSession: return .committedButSessionChanged
            case .cancelled: return .cancelled
            case .storageCapacity: return .storageFailure
            case .revisionRegression, .reconciliationRequired: return .metadataChanged
            case .missingMaterialization: return .missingMaterialization
            case .stalePage, .conflictingEvent: return .reconciliationRequired
            case .malformedMetadata, .invalidParent: return .protocolFailure
            }
        }
        if let failure = error as? MutationQueueFailure {
            switch failure {
            case .syncBaseUnavailable: return .checkpointRequired
            case .unauthenticated: return .authenticationRequired
            case .scopeMismatch: return .scopeMismatch
            case .staleSession, .committedButSessionChanged: return .committedButSessionChanged
            case .cancelled: return .cancelled
            case .concurrentExecution: return .alreadyRunning
            case .recoveryLimit: return .recoveryLimitReached
            case .reconciliationRequired, .invalidCheckpoint: return .reconciliationRequired
            case .transport(let value): return classify(value)
            default: return .storageFailure
            }
        }
        if let failure = error as? LibraryFailure {
            switch failure {
            case .unauthenticated, .authenticationRejected, .credentialUnavailable,
                .invalidCredential:
                return .authenticationRequired
            case .deviceRevoked: return .deviceRevoked
            case .staleSession: return .committedButSessionChanged
            case .originMismatch: return .scopeMismatch
            case .offline, .dnsFailure: return .offline
            case .timeout, .serverUnavailable: return .serverUnavailable
            case .cancelled: return .cancelled
            default: return .protocolFailure
            }
        }
        if error is CancellationError { return .cancelled }
        return .protocolFailure
    }
}
