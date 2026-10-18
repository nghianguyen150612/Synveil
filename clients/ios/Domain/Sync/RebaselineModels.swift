import Foundation

struct RebaselineBootstrapId: Hashable, Sendable {
    let rawValue: String
    static func validated(_ value: String, bridge: any RustBridgeProtocol) async throws -> Self {
        guard try await bridge.validateNodeID(value) else {
            throw RebaselineFailure.protocolFailure
        }
        return Self(rawValue: value)
    }
}

struct RebaselineGeneration: Equatable, Sendable {
    let value: ClientMutationDecimal
}

enum RebaselineServerState: String, Codable, Sendable {
    case open = "OPEN", completed = "COMPLETED", aborted = "ABORTED", expired = "EXPIRED"
}

struct RebaselineBootstrap: Equatable, Sendable {
    let id: RebaselineBootstrapId
    let scope: ClientMutationScope
    let state: RebaselineServerState
    let generation: RebaselineGeneration
    let position: SyncJournalPosition
    let itemCount: Int
    let createdAt: Date, expiresAt: Date, completedAt: Date?
    func sameManifest(as other: Self) -> Bool {
        id == other.id && scope == other.scope && generation == other.generation
            && position == other.position && itemCount == other.itemCount
            && createdAt == other.createdAt && expiresAt == other.expiresAt
    }
}

struct RebaselineSnapshotContent: Equatable, Sendable {
    let byteLength: ClientMutationDecimal
    let sha256: String
}

/// Logical manifest metadata only. No fabricated live timestamps or content availability.
struct RebaselineSnapshotNode: Equatable, Sendable {
    let id: NodeId, parentId: NodeId?
    let name: String
    let kind: NodeKind, state: NodeState
    let revision: NodeRevision
    let currentVersionId: FileVersionId?
    let content: RebaselineSnapshotContent?
}

struct RebaselineCompletionEvidence: Equatable, Sendable, CustomStringConvertible,
    CustomDebugStringConvertible
{
    let bootstrap: RebaselineBootstrap
    let token: String
    var description: String { "[REDACTED_REBASELINE_EVIDENCE]" }
    var debugDescription: String { description }
}

struct RebaselineManifestPage: Equatable, Sendable {
    let bootstrap: RebaselineBootstrap
    let nodes: [RebaselineSnapshotNode]
    let nextCursor: String?
    let evidence: RebaselineCompletionEvidence?
    let responseBody: Data, canonicalData: Data
}

struct RebaselineCompletionResult: Equatable, Sendable {
    let bootstrap: RebaselineBootstrap
    let position: SyncJournalPosition
    let updatedAt: Date
    let replayed: Bool
    let responseBody: Data
}

enum RebaselineStageState: String, Sendable {
    case startUnknown = "START_UNKNOWN", bootstrapOpen = "BOOTSTRAP_OPEN", downloading =
        "DOWNLOADING"
    case terminalReceived = "TERMINAL_RECEIVED", prepared = "PREPARED_FOR_HANDOFF"
    case completionInFlight = "SERVER_COMPLETION_IN_FLIGHT", completionConfirmed =
        "SERVER_COMPLETION_CONFIRMED"
    case activeComplete = "ACTIVE_COMPLETE", expired = "EXPIRED", blocked = "BLOCKED"
    case outcomeUnknown = "OUTCOME_UNKNOWN", reconciliationRequired = "RECONCILIATION_REQUIRED"
}
enum RebaselineRecoveryAction: Equatable, Sendable {
    case resumeDownload, prepare, complete, recoverOriginalCompletion, activateConfirmed,
        repeatStart
}

struct RebaselineProgress: Equatable, Sendable {
    let state: RebaselineStageState
    let pages: Int, stagedNodes: Int, expectedNodes: Int
    let position: SyncJournalPosition?
    var recoveryAction: RebaselineRecoveryAction? {
        switch state {
        case .bootstrapOpen, .downloading: .resumeDownload
        case .terminalReceived: .prepare
        case .prepared: .complete
        case .outcomeUnknown, .completionInFlight: .recoverOriginalCompletion
        case .completionConfirmed: .activateConfirmed
        case .startUnknown: .repeatStart
        default: nil
        }
    }
}

enum RebaselineFailure: Error, Equatable, Sendable {
    case confirmationRequired, protocolFailure, scopeMismatch, generationMismatch, expired
    case countMismatch, invalidGraph, unverifiedManifest, capacity, alreadyRunning
    case incompatibleSync, incompatibleMutations, reconciliationRequired, staleSession, cancelled
    case storage(MutationQueueFailure), transport(LibraryFailure)
    static func classify(_ error: Error) -> Self {
        if let failure = error as? Self { return failure }
        if error is CancellationError { return .cancelled }
        if let failure = error as? LibraryFailure { return .transport(failure) }
        if let failure = error as? SynveilTransportError {
            switch failure {
            case .offline: return .transport(.offline)
            case .dnsFailure: return .transport(.dnsFailure)
            case .timeout: return .transport(.timeout)
            case .tlsError: return .transport(.tlsFailure)
            case .redirectRejected: return .transport(.redirectRejected)
            case .bodyLimitExceeded: return .capacity
            case .cancelled: return .cancelled
            default: return .protocolFailure
            }
        }
        if let failure = error as? MutationQueueFailure {
            switch failure {
            case .capacity, .diskFull: return .capacity
            case .concurrentExecution: return .alreadyRunning
            case .scopeMismatch: return .scopeMismatch
            case .staleSession: return .staleSession
            default: return .storage(failure)
            }
        }
        return .protocolFailure
    }
}

enum RebaselinePolicy {
    static let pageSize = 200, maximumPageSize = 1000, maximumPagesPerRun = 16
    static let maximumRows = 16384, maximumPages = 1024, maximumStoredBytes = 16 * 1024 * 1024
    static let maximumResponseBytes = 2 * 1024 * 1024, maximumNodeBytes = 16384
    static func validOpaque(_ value: String, maximum: Int) -> Bool {
        !value.isEmpty && value.utf8.count <= maximum
            && value.utf8.allSatisfy { (33...126).contains($0) }
    }
    static func completionBody(_ token: String) throws -> Data {
        guard validOpaque(token, maximum: 336) else { throw RebaselineFailure.protocolFailure }
        let bytes = try JSONSerialization.data(
            withJSONObject: ["completion_token": token], options: [.sortedKeys])
        guard bytes.count <= 2048 else { throw RebaselineFailure.capacity }
        return bytes
    }
}
