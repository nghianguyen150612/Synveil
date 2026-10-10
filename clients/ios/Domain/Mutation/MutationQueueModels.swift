import Foundation

enum MutationQueueState: String, CaseIterable, Sendable {
    case pending = "PENDING", submitting = "SUBMITTING", outcomeUnknown = "OUTCOME_UNKNOWN"
    case applied = "APPLIED", conflict = "CONFLICT", blockedRebaseline = "BLOCKED_REBASELINE"
    case failedPermanent = "FAILED_PERMANENT"

    var outstanding: Bool { self != .applied && self != .failedPermanent }
    /// Ordinary attempt lifecycle. Explicit unknown recovery is guarded separately in SQLite.
    func permits(_ destination: Self) -> Bool {
        self == .pending && destination == .submitting
            || self == .submitting
                && [.applied, .conflict, .outcomeUnknown, .blockedRebaseline, .failedPermanent]
                    .contains(destination)
    }
}

enum MutationQueueFailure: Error, Equatable, Sendable {
    case unavailable, databaseOpen, busy, diskFull, corrupt, io, unsupportedSchema, malformedRecord
    case invalidCheckpoint, syncBaseUnavailable, reconciliationRequired, capacity,
        dependencyConflict
    case duplicateIdentity, notFound, invalidTransition, ownershipRequired, scopeMismatch,
        staleSession
    case unauthenticated, cancelled, invalidLimit, invalidOperation, transport(LibraryFailure)
    case concurrentExecution, recoveryLimit
    /// The write committed but its initiating session/task can no longer publish an actionable row.
    /// Read-back using a newly validated exact scope is required; this never means rollback.
    case commitAcknowledgementLost
    case committedButSessionChanged
}

enum MutationQueuePolicy {
    static let encodingVersion = 1
    static let maximumOutstandingPerScope = 256
    static let maximumStoredBytes = 16 * 1024 * 1024
    static let maximumRecords = 4096
    static let maximumReadBatch = 100
    static let maximumCheckpointBytes = 16 * 1024
    static let maximumRecoveryAttempts = 8
}

struct MutationQueueRecord: Equatable, Sendable, CustomStringConvertible,
    CustomDebugStringConvertible
{
    let mutation: PreparedClientMutation
    let enqueueOrder: Int64
    let createdAt: Date
    let state: MutationQueueState
    let attempt: MutationAttemptMetadata?
    let evidence: MutationOutcomeEvidence?
    var description: String { "[REDACTED_MUTATION_QUEUE_RECORD]" }
    var debugDescription: String { description }
}

struct MutationAttemptMetadata: Equatable, Sendable {
    let id: String
    let owner: String
    let startedAt: Date
    let dispatchRecorded: Bool
}

/// Storage-only evidence format. Response projections are strictly decoded again on read.
/// No transport diagnostics, headers, credentials, or server error messages are retained.
struct MutationOutcomeEvidence: Equatable, Sendable, Codable, CustomStringConvertible,
    CustomDebugStringConvertible
{
    enum Category: String, Codable, Sendable {
        case applied, conflict, rebaseline, identityConflict, permanentRejection, outcomeUnknown
    }
    let category: Category
    let responseStatus: Int?
    let responseBody: Data?
    let rejection: String?
    let uncertainty: String?
    var description: String { "[REDACTED_MUTATION_EVIDENCE]" }
    var debugDescription: String { description }
}

enum MutationEnqueueResult: Equatable, Sendable {
    case enqueued(MutationQueueRecord), existing(MutationQueueRecord), failed(MutationQueueFailure)
}

enum MutationQueueReadResult: Equatable, Sendable {
    case records([MutationQueueRecord]), failed(MutationQueueFailure)
}

enum MutationQueueLookupResult: Equatable, Sendable {
    case record(MutationQueueRecord), missing, failed(MutationQueueFailure)
}

enum MutationQueueCountResult: Equatable, Sendable {
    case count(Int), missing, failed(MutationQueueFailure)
}

enum MutationRecoveryResult: Equatable, Sendable {
    case recovered(Int), failed(MutationQueueFailure)
}

/// Raw immutable storage DTO; complete validation is required before exposing a prepared operation.
struct StoredMutationRecord: Sendable {
    let scope: ClientMutationScope
    let mutationId: String
    let epoch: String
    let sequence: String
    let kind: String
    let payload: Data
    let request: Data
    let encodingVersion: Int
    let order: Int64
    let createdAt: Date
    let state: MutationQueueState
    let attempt: MutationAttemptMetadata?
    let evidence: Data?
}

struct MutationHistoricalAttempt: Equatable, Sendable, CustomStringConvertible,
    CustomDebugStringConvertible
{
    let attempt: MutationAttemptMetadata
    let evidence: Data
    var description: String { "[REDACTED_MUTATION_ATTEMPT_HISTORY]" }
    var debugDescription: String { description }
}

protocol MutationQueueStorageProtocol: Sendable {
    func bindSession(scope: ClientMutationScope, credentialId: String) async throws
    func quarantineSessions() async throws
    func syncBase(scope: ClientMutationScope) async throws -> StoredSyncBase?
    func persistCheckpoint(_ checkpoint: SyncCheckpoint) async throws -> SyncBaseStatus
    func invalidateBase(scope: ClientMutationScope) async throws
    func enqueue(_ mutation: PreparedClientMutation, payload: Data) async throws -> (
        StoredMutationRecord, Bool
    )
    func records(scope: ClientMutationScope, limit: Int) async throws -> [StoredMutationRecord]
    func activityRecords(scope: ClientMutationScope, limit: Int) async throws
        -> [StoredMutationRecord]
    func record(scope: ClientMutationScope, id: String) async throws -> StoredMutationRecord?
    func recoverInterruptedOperations() async throws -> Int
    func beginAttempt(scope: ClientMutationScope, id: String, owner: String, attemptId: String)
        async throws -> StoredMutationRecord
    func beginRecoveryAttempt(
        scope: ClientMutationScope, id: String, owner: String, attemptId: String
    )
        async throws -> StoredMutationRecord
    func attemptHistory(scope: ClientMutationScope, id: String) async throws
        -> [MutationHistoricalAttempt]
    func authorizeAttempt(_ mutation: PreparedClientMutation, owner: String, attemptId: String)
        async throws
    func finishAttempt(
        scope: ClientMutationScope, id: String, owner: String, attemptId: String,
        state: MutationQueueState, evidence: Data) async throws
}

@MainActor
protocol DurableMutationQueueProtocol {
    func enqueue(_ mutation: PreparedClientMutation) async -> MutationEnqueueResult
    func pending(scope: ClientMutationScope, limit: Int) async -> MutationQueueReadResult
    func activity(scope: ClientMutationScope, limit: Int) async -> MutationQueueReadResult
    func get(scope: ClientMutationScope, mutationId: ClientMutationId) async
        -> MutationQueueLookupResult
    func recoverInterruptedOperations() async -> MutationRecoveryResult
}
