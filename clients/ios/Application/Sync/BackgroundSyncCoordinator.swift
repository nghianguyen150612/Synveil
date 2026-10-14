import Foundation

@MainActor
protocol BackgroundSyncWorkProtocol {
    func capture() async throws -> LibraryRequestScope
    func validate(_ session: LibraryRequestScope) async throws
    func libraries(session: LibraryRequestScope, after: String?) async throws -> [LibraryId]
    /// Must acquire existing durable run ownership before starting authorization verification.
    func verify(session: LibraryRequestScope, library: LibraryId) async throws
    func synchronize(session: LibraryRequestScope, library: LibraryId) async -> InboundSyncRunResult
    func confirmation(session: LibraryRequestScope, library: LibraryId) async throws
        -> BackgroundSyncConfirmation?
}

/// One delivery-scoped budget. Every metadata request, including materialization and ACK, checks
/// this gate. The monotonic clock and system constraints are independently injectable.
@MainActor
final class BackgroundSyncRequestGate {
    private let uptime: @MainActor () -> TimeInterval
    private let conditionsAllow: @MainActor () -> Bool
    private var permit: (@MainActor () -> Bool)?
    private var deadline: TimeInterval = 0
    init(
        uptime: @escaping @MainActor () -> TimeInterval = { ProcessInfo.processInfo.systemUptime },
        conditionsAllow: @escaping @MainActor () -> Bool
    ) {
        self.uptime = uptime
        self.conditionsAllow = conditionsAllow
    }
    func open(
        remainingTime: TimeInterval = BackgroundSyncLimits.runtime,
        permit: @escaping @MainActor () -> Bool
    ) {
        deadline = uptime() + max(0, min(BackgroundSyncLimits.runtime, remainingTime))
        self.permit = permit
    }
    func close() { permit = nil }
    var canStartRequest: Bool {
        permit?() == true && conditionsAllow() && !Task.isCancelled
            && deadline - uptime()
                >= BackgroundSyncLimits.resourceTimeout + BackgroundSyncLimits.cleanupReserve
    }
    func check() throws {
        try Task.checkCancellation()
        guard canStartRequest else { throw CancellationError() }
    }
}

/// Thin, sequential orchestration over the existing inbound engine. No recovery, outbound queue,
/// rebaseline or file-transfer capability is supplied to this object.
@MainActor
final class BackgroundSyncCoordinator {
    private let policy: any BackgroundSyncPolicyStorage
    private let work: any BackgroundSyncWorkProtocol
    let gate: BackgroundSyncRequestGate
    init(
        policy: any BackgroundSyncPolicyStorage, work: any BackgroundSyncWorkProtocol,
        gate: BackgroundSyncRequestGate
    ) {
        self.policy = policy
        self.work = work
        self.gate = gate
    }

    func run(delivery: BackgroundRefreshDelivery) async -> BackgroundSyncOutcome {
        let saved = policy.read()
        guard saved.enabled else { return BackgroundSyncOutcome(state: .disabled) }
        gate.open(remainingTime: delivery.remainingTime) { [policy] in
            let current = policy.read()
            return !delivery.isExpired && current.enabled && current.grant == saved.grant
                && current.identity == saved.identity && current.network == saved.network
        }
        defer { gate.close() }
        var outcome = BackgroundSyncOutcome(state: .completed)
        do {
            try gate.check()
            let session = try await work.capture()
            guard BackgroundSyncIdentity(session) == saved.identity else {
                return BackgroundSyncOutcome(state: .authenticationRequired)
            }
            try gate.check()
            let libraries = try await work.libraries(session: session, after: saved.afterLibrary)
            guard libraries.count <= BackgroundSyncLimits.maximumLibraries,
                Set(libraries).count == libraries.count
            else { return BackgroundSyncOutcome(state: .failed) }
            guard let first = libraries.first else {
                return BackgroundSyncOutcome(state: .deferred)
            }
            try gate.check()
            var attempted = policy.read()
            guard attempted.enabled, attempted.grant == saved.grant,
                attempted.identity == saved.identity
            else { return BackgroundSyncOutcome(state: .invalidated) }
            attempted.afterLibrary = first.rawValue
            guard policy.write(attempted) else { return BackgroundSyncOutcome(state: .failed) }
            try await work.verify(session: session, library: first)
            for library in libraries {
                try gate.check()
                try await work.validate(session)
                try gate.check()
                // Advance before attempting a library, including persistent recovery failures.
                // Do not overwrite a newer user decision after an awaited operation.
                var next = policy.read()
                guard next.enabled, next.grant == saved.grant, next.identity == saved.identity
                else { return BackgroundSyncOutcome(state: .invalidated) }
                next.afterLibrary = library.rawValue
                guard policy.write(next) else { return BackgroundSyncOutcome(state: .failed) }
                let result = await work.synchronize(session: session, library: library)
                guard
                    (0...InboundSyncRunConfiguration.background.maximumPages).contains(
                        result.progress.pagesProcessed),
                    (0...InboundSyncRunConfiguration.background.maximumEvents).contains(
                        result.progress.eventsApplied)
                else { return replacing(outcome, state: .failed) }
                try await work.validate(session)
                guard !delivery.isExpired, !Task.isCancelled,
                    policy.read().grant == saved.grant, policy.read().enabled
                else { throw CancellationError() }
                outcome.pages += result.progress.pagesProcessed
                outcome.events += result.progress.eventsApplied
                if result.progress.pagesProcessed > 0 {
                    // Publication requires SQLite readback, never just a returned success flag.
                    if let confirmation = try await work.confirmation(
                        session: session, library: library)
                    {
                        outcome.confirmation = confirmation
                    } else {
                        return BackgroundSyncOutcome(state: .failed)
                    }
                }
                switch result.reason {
                case .upToDate, .progressed: break
                case .moreWork:
                    if outcome.state == .completed { outcome = replacing(outcome, state: .partial) }
                case .alreadyRunning: return replacing(outcome, state: .deferred)
                case .offline, .serverUnavailable: return replacing(outcome, state: .offline)
                case .authenticationRequired, .deviceRevoked:
                    return replacing(outcome, state: .authenticationRequired)
                case .committedButSessionChanged, .scopeMismatch:
                    return replacing(outcome, state: .invalidated)
                case .cancelled:
                    return replacing(outcome, state: delivery.isExpired ? .expired : .partial)
                case .checkpointRequired, .awaitingAckRecovery, .reconciliationRequired,
                    .rebaselineRequired, .recoveryLimitReached, .recoveryConfirmationRequired,
                    .metadataChanged, .missingMaterialization:
                    outcome = replacing(outcome, state: .recoveryRequired)
                case .storageFailure, .protocolFailure, .invalidConfiguration:
                    // A per-library error must not permanently starve the next eligible library.
                    outcome = replacing(outcome, state: .failed)
                }
            }
            return outcome
        } catch {
            if delivery.isExpired { return replacing(outcome, state: .expired) }
            if !policy.read().enabled || policy.read().grant != saved.grant {
                return replacing(outcome, state: .invalidated)
            }
            if error is CancellationError || Task.isCancelled {
                return replacing(outcome, state: outcome.pages > 0 ? .partial : .deferred)
            }
            switch InboundSyncCoordinator.classify(error) {
            case .alreadyRunning: return replacing(outcome, state: .deferred)
            case .offline, .serverUnavailable: return replacing(outcome, state: .offline)
            case .authenticationRequired, .deviceRevoked:
                return replacing(outcome, state: .authenticationRequired)
            case .committedButSessionChanged, .scopeMismatch:
                return replacing(outcome, state: .invalidated)
            case .rebaselineRequired, .reconciliationRequired, .checkpointRequired,
                .awaitingAckRecovery, .recoveryLimitReached:
                return replacing(outcome, state: .recoveryRequired)
            default: return replacing(outcome, state: .failed)
            }
        }
    }
    private func replacing(_ value: BackgroundSyncOutcome, state: BackgroundSyncState)
        -> BackgroundSyncOutcome
    {
        BackgroundSyncOutcome(
            state: state, pages: value.pages, events: value.events, confirmation: value.confirmation
        )
    }
}
