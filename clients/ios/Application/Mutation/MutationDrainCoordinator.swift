import Foundation

/// Counts only committed outcomes that still belong to the invoking authenticated session.
struct MutationDrainSummary: Equatable, Sendable {
    var attempted = 0
    var applied = 0
    var conflicts = 0
    var identityConflicts = 0
    var permanentRejections = 0
    var blockedRebaseline = 0
    var unknown = 0
    var localPreDispatchFailures = 0
}

enum MutationDrainStopReason: Equatable, Sendable {
    case queue(MutationQueueFailure)
    case resultPersistenceFailed(MutationQueueFailure)
    case submission(ClientMutationFailure)
    case unknownRequiresReconciliation, conflictRequiresReview, rebaselineRequired
    case mutationIdConflict
}

enum MutationDrainResult: Equatable, Sendable {
    case completed(MutationDrainSummary)
    case stopped(MutationDrainStopReason, MutationDrainSummary)
}

typealias MutationReconciliationResult = MutationDrainResult

/// Trusted application orchestration only. Composition never invokes this service. The scope's
/// oldest outstanding record is processed sequentially; ambiguity/conflict stops the entire scope.
/// SQLite remains the final authority across separate coordinator/queue instances.
@MainActor
final class MutationDrainCoordinator {
    private let queue: DurableMutationQueue
    private let provider: any AuthenticatedClientMutationRequestProviderProtocol
    private let bridge: any RustBridgeProtocol
    private var active = false

    init(
        queue: DurableMutationQueue,
        provider: any AuthenticatedClientMutationRequestProviderProtocol,
        bridge: any RustBridgeProtocol
    ) {
        self.queue = queue
        self.provider = provider
        self.bridge = bridge
    }

    func drain(scope: ClientMutationScope, maximumOperations: Int) async -> MutationDrainResult {
        var summary = MutationDrainSummary()
        guard (1...MutationQueuePolicy.maximumReadBatch).contains(maximumOperations) else {
            return .stopped(.queue(.invalidLimit), summary)
        }
        guard !active else { return .stopped(.queue(.concurrentExecution), summary) }
        active = true
        defer { active = false }
        do {
            let session = try await queue.capture(scope: scope)
            _ = try await queue.persistedBase(scope: scope)
            for _ in 0..<maximumOperations {
                try await queue.validateExecutionSession(session)
                // Requery after each committed result. Terminal rows cannot starve pending rows.
                let records = await queue.pending(scope: scope, limit: 1)
                let row: MutationQueueRecord
                switch records {
                case .failed(let failure): return .stopped(.queue(failure), summary)
                case .records(let rows):
                    guard let first = rows.first else {
                        try await queue.validateExecutionSession(session)
                        return .completed(summary)
                    }
                    row = first
                }
                guard row.state == .pending else {
                    return .stopped(stopReason(row.state), summary)
                }
                let lease = try await queue.acquireAttempt(
                    scope: scope, mutationId: row.mutation.id)
                if let stop = await execute(lease, summary: &summary) {
                    return .stopped(stop, summary)
                }
            }
            try await queue.validateExecutionSession(session)
            return .completed(summary)
        } catch { return .stopped(.queue(DurableMutationQueue.classify(error)), summary) }
    }

    /// A single explicit same-ID attempt. No retry loop, checkpoint refresh or intent reconstruction.
    /// The inspected server replays scoped terminal operations before epoch/retention validation;
    /// otherwise it rejects an unusable original base. Local reconciliation-required bases block.
    func reconcileUnknown(scope: ClientMutationScope, mutationId: ClientMutationId) async
        -> MutationReconciliationResult
    {
        var summary = MutationDrainSummary()
        guard !active else { return .stopped(.queue(.concurrentExecution), summary) }
        active = true
        defer { active = false }
        do {
            let lease = try await queue.acquireRecoveryAttempt(scope: scope, mutationId: mutationId)
            if let stop = await execute(lease, summary: &summary) { return .stopped(stop, summary) }
            return .completed(summary)
        } catch { return .stopped(.queue(DurableMutationQueue.classify(error)), summary) }
    }

    private func execute(_ lease: MutationSubmissionLease, summary: inout MutationDrainSummary)
        async -> MutationDrainStopReason?
    {
        let repository = AuthenticatedClientMutationRepository(
            provider: MutationLeaseRequestProvider(provider: provider, session: lease.session),
            bridge: bridge, authorizer: DurableMutationAttemptAuthorizer(queue: queue, lease: lease)
        )
        let result = await repository.submit(lease.mutation)
        let preDispatchFailure = !repository.didDispatch
        summary.attempted += 1
        do {
            // A cancelled caller cannot prevent bounded local uncertainty persistence. This task
            // performs one SQLite finish, never a POST, and still fences lease/session ownership.
            let persistence = Task { @MainActor [queue] in
                try await queue.finish(
                    lease, result: result, preDispatchFailure: preDispatchFailure)
            }
            try await persistence.value
            try await queue.validateExecutionSession(lease.session)
        } catch {
            // Last committed SUBMITTING/terminal state remains authoritative. Never resend HTTP
            // or claim durable success because transport succeeded or COMMIT acknowledgement failed.
            if case .failed(let failure) = result,
                failure == .authenticationRejected || failure == .deviceRevoked
            {
                return .submission(failure)
            }
            let failure = DurableMutationQueue.classify(error)
            switch failure {
            case .cancelled, .staleSession, .committedButSessionChanged, .ownershipRequired:
                return .queue(failure)
            default: return .resultPersistenceFailed(failure)
            }
        }
        switch result {
        case .applied:
            summary.applied += 1
            return nil
        case .conflict:
            summary.conflicts += 1
            return .conflictRequiresReview
        case .mutationIdConflict:
            summary.identityConflicts += 1
            return .mutationIdConflict
        case .rebaselineRequired:
            summary.blockedRebaseline += 1
            return .rebaselineRequired
        case .failed(.permanentRejection(let rejection)):
            summary.permanentRejections += 1
            return .submission(.permanentRejection(rejection))
        case .failed(let failure):
            if preDispatchFailure {
                summary.localPreDispatchFailures += 1
            } else {
                summary.unknown += 1
            }
            return .submission(failure)
        case .outcomeUnknown(let failure):
            summary.unknown += 1
            return .submission(failure)
        }
    }

    private func stopReason(_ state: MutationQueueState) -> MutationDrainStopReason {
        switch state {
        case .outcomeUnknown: return .unknownRequiresReconciliation
        case .conflict: return .conflictRequiresReview
        case .blockedRebaseline: return .rebaselineRequired
        default: return .queue(.concurrentExecution)
        }
    }
}

/// Prevents an old lease borrowing a provider's newly captured credential/session. Both transport
/// and authorization must use the opaque session originally bound by the queue.
@MainActor
private final class MutationLeaseRequestProvider: AuthenticatedClientMutationRequestProviderProtocol
{
    private let provider: any AuthenticatedClientMutationRequestProviderProtocol
    private let session: LibraryRequestScope
    init(
        provider: any AuthenticatedClientMutationRequestProviderProtocol,
        session: LibraryRequestScope
    ) {
        self.provider = provider
        self.session = session
    }
    func begin() async throws -> LibraryRequestScope {
        try await provider.validate(session)
        return session
    }
    func validate(_ scope: LibraryRequestScope) async throws {
        guard scope.sameSession(as: session) else { throw ClientMutationFailure.staleSession }
        try await provider.validate(session)
    }
    func submitMutation(
        _ mutation: PreparedClientMutation, scope: LibraryRequestScope,
        onDispatch: () -> Void
    ) async throws -> HTTPTransportResponse {
        try await validate(scope)
        return try await provider.submitMutation(mutation, scope: session, onDispatch: onDispatch)
    }
    func handle(_ failure: LibraryFailure, scope: LibraryRequestScope) {
        guard scope.sameSession(as: session) else { return }
        provider.handle(failure, scope: session)
    }
}
