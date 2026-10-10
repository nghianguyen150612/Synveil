import Foundation

/// Lease is a capability created only by the queue controller. Session and owner are memory-only;
/// the persisted attempt ID is evidence, never sufficient authority by itself.
@MainActor
final class MutationSubmissionLease {
    let mutation: PreparedClientMutation
    let session: LibraryRequestScope
    fileprivate let owner: String
    fileprivate let attemptId: String
    fileprivate init(
        mutation: PreparedClientMutation, session: LibraryRequestScope, owner: String,
        attemptId: String
    ) {
        self.mutation = mutation
        self.session = session
        self.owner = owner
        self.attemptId = attemptId
    }
}

/// Module-internal infrastructure interface; never injected into SwiftUI. No method invokes POST.
@MainActor
final class DurableMutationQueue: DurableMutationQueueProtocol {
    private let store: any MutationQueueStorageProtocol
    private let provider: any AuthenticatedSyncCheckpointRequestProviderProtocol
    private let codec: MutationPersistenceCodec
    private let checkpointDecoder: SyncCheckpointResponseDecoder
    private var owner = UUID().uuidString
    private var sessions: [(ClientMutationScope, LibraryRequestScope)] = []
    private var quarantineTask: Task<Void, Never>?
    private var quarantineFailed = false

    init(
        store: any MutationQueueStorageProtocol,
        provider: any AuthenticatedSyncCheckpointRequestProviderProtocol,
        bridge: any RustBridgeProtocol
    ) {
        self.store = store
        self.provider = provider
        codec = MutationPersistenceCodec(bridge: bridge)
        checkpointDecoder = SyncCheckpointResponseDecoder(bridge: bridge)
    }

    /// Called synchronously when SessionController leaves authentication. Durable records remain
    /// quarantined. Immediate capability invalidation precedes asynchronous SQLite quarantine.
    func invalidateSession() {
        owner = UUID().uuidString
        sessions.removeAll()
        let prior = quarantineTask
        let store = store
        quarantineTask = Task { [weak self] in
            await prior?.value
            do { try await store.quarantineSessions() } catch { self?.quarantineFailed = true }
        }
    }

    func capture(scope: ClientMutationScope) async throws -> LibraryRequestScope {
        try Task.checkCancellation()
        await quarantineTask?.value
        guard !quarantineFailed else { throw MutationQueueFailure.unavailable }
        try await codec.validateScope(scope)
        let previous = sessions.first(where: { $0.0 == scope })?.1
        if let previous {
            do { try await provider.validate(previous) } catch {
                invalidateSession()
                await quarantineTask?.value
                throw MutationQueueFailure.staleSession
            }
        }
        let session = try await provider.begin()
        if let previous, !previous.sameSession(as: session) {
            invalidateSession()
            await quarantineTask?.value
            throw MutationQueueFailure.staleSession
        }
        guard session.matches(scope) else { throw MutationQueueFailure.scopeMismatch }
        try await provider.validate(session)
        try await store.bindSession(scope: scope, credentialId: session.credentialIdentifier)
        try await provider.validate(session)
        if !sessions.contains(where: { $0.0 == scope }) { sessions.append((scope, session)) }
        return session
    }

    private func fence(_ session: LibraryRequestScope) async throws {
        do {
            try await provider.validate(session)
            try Task.checkCancellation()
        } catch {
            if !(error is CancellationError), !Task.isCancelled { invalidateSession() }
            throw error
        }
    }

    func persistedBase(scope: ClientMutationScope) async throws -> ClientMutationBase {
        let session = try await capture(scope: scope)
        let base = try await verifiedBase(scope: scope)
        try await fence(session)
        return base
    }

    private func verifiedBase(scope: ClientMutationScope) async throws -> ClientMutationBase {
        guard let stored = try await store.syncBase(scope: scope) else {
            throw MutationQueueFailure.syncBaseUnavailable
        }
        guard stored.scope == scope else { throw MutationQueueFailure.scopeMismatch }
        guard stored.status == .verified else {
            throw stored.status == .invalid
                ? MutationQueueFailure.invalidCheckpoint : .reconciliationRequired
        }
        let checkpoint = try await checkpointDecoder.decode(
            HTTPTransportResponse(
                statusCode: 200, headers: ["Content-Type": "application/json"],
                body: stored.responseBody), scope: scope)
        guard checkpoint.base.epoch.rawValue == stored.epoch,
            checkpoint.base.sequence.rawValue == stored.sequence
        else {
            throw MutationQueueFailure.invalidCheckpoint
        }
        return checkpoint.base
    }

    func persist(checkpoint: SyncCheckpoint, session: LibraryRequestScope) async throws
        -> SyncCheckpointPreparationResult
    {
        guard session.matches(checkpoint.base.scope) else {
            throw MutationQueueFailure.scopeMismatch
        }
        // Revalidate supplied provenance; valid-looking manually constructed values are insufficient.
        let verified = try await checkpointDecoder.decode(
            HTTPTransportResponse(
                statusCode: 200, headers: ["Content-Type": "application/json"],
                body: checkpoint.responseBody), scope: checkpoint.base.scope)
        guard verified == checkpoint else { throw MutationQueueFailure.invalidCheckpoint }
        try await fence(session)
        let status: SyncBaseStatus
        do { status = try await store.persistCheckpoint(checkpoint) } catch MutationQueueFailure
            .commitAcknowledgementLost
        {
            guard let readback = try await store.syncBase(scope: checkpoint.base.scope),
                readback.responseBody == checkpoint.responseBody
            else { throw MutationQueueFailure.commitAcknowledgementLost }
            status = readback.status
        }
        do { try await fence(session) } catch {
            throw MutationQueueFailure.committedButSessionChanged
        }
        return status == .verified ? .prepared(checkpoint.base) : .failed(.reconciliationRequired)
    }

    func invalidateBase(scope: ClientMutationScope, session: LibraryRequestScope) async throws {
        try await fence(session)
        try await store.invalidateBase(scope: scope)
        do { try await fence(session) } catch {
            throw MutationQueueFailure.committedButSessionChanged
        }
    }

    func enqueue(_ mutation: PreparedClientMutation) async -> MutationEnqueueResult {
        do {
            let session = try await capture(scope: mutation.base.scope)
            let base = try await verifiedBase(scope: mutation.base.scope)
            guard base == mutation.base else { throw MutationQueueFailure.reconciliationRequired }
            let reconstructed = try await PreparedClientMutation(
                id: mutation.id, base: mutation.base,
                intent: mutation.payload.intent, bridge: codec.bridge)
            guard mutation == reconstructed else { throw MutationQueueFailure.invalidOperation }
            let payload = try codec.payloadBytes(mutation)
            try await fence(session)
            let stored: (StoredMutationRecord, Bool)
            do {
                stored = try await store.enqueue(mutation, payload: payload)
            } catch MutationQueueFailure.commitAcknowledgementLost {
                guard
                    let row = try await store.record(
                        scope: mutation.base.scope, id: mutation.id.rawValue),
                    try await codec.rehydrate(row).mutation == mutation
                else { throw MutationQueueFailure.commitAcknowledgementLost }
                stored = (row, true)
            }
            do {
                let record = try await codec.rehydrate(stored.0)
                try await fence(session)
                return stored.1 ? .enqueued(record) : .existing(record)
            } catch {
                if Task.isCancelled || error is CancellationError
                    || error as? LibraryFailure == .staleSession
                {
                    throw MutationQueueFailure.committedButSessionChanged
                }
                throw error
            }
        } catch { return .failed(Self.classify(error)) }
    }

    func pending(scope: ClientMutationScope, limit: Int) async -> MutationQueueReadResult {
        do {
            let session = try await capture(scope: scope)
            let rows = try await store.records(scope: scope, limit: limit)
            var records: [MutationQueueRecord] = []
            for row in rows { records.append(try await codec.rehydrate(row)) }
            try await fence(session)
            return .records(records)
        } catch { return .failed(Self.classify(error)) }
    }

    func get(scope: ClientMutationScope, mutationId: ClientMutationId) async
        -> MutationQueueLookupResult
    {
        do {
            let session = try await capture(scope: scope)
            let row = try await store.record(scope: scope, id: mutationId.rawValue)
            let record: MutationQueueRecord?
            if let row { record = try await codec.rehydrate(row) } else { record = nil }
            try await fence(session)
            return record.map { .record($0) } ?? .missing
        } catch { return .failed(Self.classify(error)) }
    }

    /// Startup/local recovery is deliberately network-free and does not expose records. It only
    /// conservatively records lost attempt ownership across all scopes. Never a replay authority.
    func recoverInterruptedOperations() async -> MutationRecoveryResult {
        owner = UUID().uuidString
        do { return .recovered(try await store.recoverInterruptedOperations()) } catch {
            return .failed(Self.classify(error))
        }
    }

    func validateExecutionSession(_ session: LibraryRequestScope) async throws {
        try await fence(session)
    }

    /// Only the explicit reconciliation boundary calls this method. Normal drains use acquireAttempt.
    func acquireRecoveryAttempt(scope: ClientMutationScope, mutationId: ClientMutationId)
        async throws
        -> MutationSubmissionLease
    {
        try await acquire(scope: scope, mutationId: mutationId, recovery: true)
    }

    func acquireAttempt(scope: ClientMutationScope, mutationId: ClientMutationId) async throws
        -> MutationSubmissionLease
    {
        try await acquire(scope: scope, mutationId: mutationId, recovery: false)
    }

    private func acquire(scope: ClientMutationScope, mutationId: ClientMutationId, recovery: Bool)
        async throws -> MutationSubmissionLease
    {
        let session = try await capture(scope: scope)
        let base = try await verifiedBase(scope: scope)
        guard let original = try await store.record(scope: scope, id: mutationId.rawValue) else {
            throw MutationQueueFailure.notFound
        }
        let originalRecord = try await codec.rehydrate(original)
        guard originalRecord.mutation.base == base else {
            throw MutationQueueFailure.reconciliationRequired
        }
        if recovery {
            guard originalRecord.state == .outcomeUnknown else {
                throw MutationQueueFailure.invalidTransition
            }
            let history = try await store.attemptHistory(scope: scope, id: mutationId.rawValue)
            for previous in history {
                guard UUID(uuidString: previous.attempt.id) != nil,
                    UUID(uuidString: previous.attempt.owner) != nil,
                    previous.attempt.startedAt.timeIntervalSince1970.isFinite
                else { throw MutationQueueFailure.malformedRecord }
                let evidence = try JSONDecoder().decode(
                    MutationOutcomeEvidence.self, from: previous.evidence)
                try await codec.validateEvidence(
                    evidence, state: .outcomeUnknown, mutation: originalRecord.mutation)
            }
        }
        try await fence(session)
        let attemptId = UUID().uuidString
        let capturedOwner = owner
        let row: StoredMutationRecord
        if recovery {
            row = try await store.beginRecoveryAttempt(
                scope: scope, id: mutationId.rawValue, owner: capturedOwner, attemptId: attemptId)
        } else {
            row = try await store.beginAttempt(
                scope: scope, id: mutationId.rawValue, owner: capturedOwner, attemptId: attemptId)
        }
        let record: MutationQueueRecord
        do {
            record = try await codec.rehydrate(row)
            guard record.mutation == originalRecord.mutation else {
                throw MutationQueueFailure.malformedRecord
            }
            try await fence(session)
        } catch {
            if Task.isCancelled || error is CancellationError
                || error as? LibraryFailure == .staleSession
            {
                throw MutationQueueFailure.committedButSessionChanged
            }
            throw error
        }
        guard capturedOwner == owner else { throw MutationQueueFailure.ownershipRequired }
        return MutationSubmissionLease(
            mutation: record.mutation, session: session, owner: capturedOwner, attemptId: attemptId)
    }

    fileprivate func authorize(_ mutation: PreparedClientMutation, lease: MutationSubmissionLease)
        async throws
    {
        guard lease.owner == owner, lease.mutation == mutation else {
            throw MutationQueueFailure.ownershipRequired
        }
        try await fence(lease.session)
        let currentBase = try await verifiedBase(scope: mutation.base.scope)
        guard currentBase == mutation.base else {
            throw MutationQueueFailure.reconciliationRequired
        }
        guard
            let row = try await store.record(scope: mutation.base.scope, id: mutation.id.rawValue),
            try await codec.rehydrate(row).mutation == mutation
        else { throw MutationQueueFailure.notFound }
        try await fence(lease.session)
        guard lease.owner == owner else { throw MutationQueueFailure.ownershipRequired }
        try await store.authorizeAttempt(mutation, owner: lease.owner, attemptId: lease.attemptId)
        try await fence(lease.session)
        guard lease.owner == owner else { throw MutationQueueFailure.ownershipRequired }
    }

    func finish(
        _ lease: MutationSubmissionLease, result: ClientMutationSubmissionResult,
        preDispatchFailure: Bool = false
    )
        async throws
    {
        guard lease.owner == owner else { throw MutationQueueFailure.ownershipRequired }
        try await fence(lease.session)
        let (state, evidence) = try await codec.evidence(
            for: result, mutation: lease.mutation, preDispatchFailure: preDispatchFailure)
        try await fence(lease.session)
        guard lease.owner == owner else { throw MutationQueueFailure.ownershipRequired }
        try await store.finishAttempt(
            scope: lease.mutation.base.scope, id: lease.mutation.id.rawValue,
            owner: lease.owner, attemptId: lease.attemptId, state: state, evidence: evidence)
        do { try await fence(lease.session) } catch {
            throw MutationQueueFailure.committedButSessionChanged
        }
    }

    static func classify(_ error: any Error) -> MutationQueueFailure {
        if let failure = error as? MutationQueueFailure { return failure }
        if error is CancellationError || Task.isCancelled { return .cancelled }
        switch AuthenticatedLibraryCatalogRepository.classify(error) {
        case .unauthenticated: return .unauthenticated
        case .staleSession: return .staleSession
        case .originMismatch: return .scopeMismatch
        case .protocolFailure: return .invalidOperation
        default: return .transport(AuthenticatedLibraryCatalogRepository.classify(error))
        }
    }
}

/// Bound to one lease, consumes its durable dispatch authorization at most once. Installed only
/// in a coordinator-owned per-attempt repository; the global repository remains fail-closed.
@MainActor
final class DurableMutationAttemptAuthorizer: ClientMutationPreparationAuthorizerProtocol {
    private let queue: DurableMutationQueue
    private let lease: MutationSubmissionLease
    init(queue: DurableMutationQueue, lease: MutationSubmissionLease) {
        self.queue = queue
        self.lease = lease
    }
    func authorizePersistedSubmission(_ mutation: PreparedClientMutation) async throws {
        try await queue.authorize(mutation, lease: lease)
    }
}
