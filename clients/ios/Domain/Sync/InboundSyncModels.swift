import Foundation

struct InboundSyncRunConfiguration: Equatable, Sendable {
    let pageSize: Int
    let maximumPages: Int
    let maximumEvents: Int
    static let foreground = Self(pageSize: 200, maximumPages: 8, maximumEvents: 4096)
    static let hardMaximumPages = 64
    static let hardMaximumEvents = 4096
    var isValid: Bool {
        (1...500).contains(pageSize) && (1...Self.hardMaximumPages).contains(maximumPages)
            && (1...Self.hardMaximumEvents).contains(maximumEvents)
    }
}

enum InboundSyncPhase: Equatable, Sendable {
    case checkingLocalStatus, fetching, staged, applying, applied, acknowledging, confirmed, stopped
}

/// Publication snapshots contain positions, never signed evidence or credentials.
struct InboundSyncProgress: Equatable, Sendable {
    let phase: InboundSyncPhase
    let pagesProcessed: Int
    let eventsApplied: Int
    let locallyApplied: SyncJournalPosition?
    let serverConfirmed: SyncJournalPosition?
    let observedHighWatermark: SyncJournalPosition?
    static let initial = Self(
        phase: .checkingLocalStatus, pagesProcessed: 0, eventsApplied: 0,
        locallyApplied: nil, serverConfirmed: nil, observedHighWatermark: nil)
}

enum InboundSyncStopReason: Equatable, Sendable {
    case upToDate, progressed, moreWork, checkpointRequired, awaitingAckRecovery
    case reconciliationRequired, rebaselineRequired, authenticationRequired, deviceRevoked
    case offline, serverUnavailable, storageFailure, protocolFailure, cancelled
    case committedButSessionChanged, invalidConfiguration, scopeMismatch, alreadyRunning
    case recoveryLimitReached, recoveryConfirmationRequired
    case metadataChanged, missingMaterialization
}

struct SyncRecoveryAction: Equatable, Sendable {
    let position: SyncJournalPosition
    let attempts: Int
    var canRecover: Bool { attempts < MutationQueuePolicy.maximumRecoveryAttempts }
}

struct InboundSyncStatus: Equatable, Sendable {
    let progress: InboundSyncProgress
    let completeness: NodeProjectionCompleteness?
    let pendingState: InboundSyncPageState?
    let recovery: SyncRecoveryAction?
    let stopReason: InboundSyncStopReason?
}

struct InboundSyncRunResult: Equatable, Sendable {
    let reason: InboundSyncStopReason
    let progress: InboundSyncProgress
    let recovery: SyncRecoveryAction?
}

@MainActor
protocol InboundSyncCoordinatorProtocol {
    /// Local authenticated identity capture only; does not call a checkpoint endpoint.
    func scope(libraryId: LibraryId) async throws -> ClientMutationScope
    func status(scope: ClientMutationScope) async -> InboundSyncStatus
    func synchronize(
        scope: ClientMutationScope, configuration: InboundSyncRunConfiguration,
        progress: @MainActor (InboundSyncProgress) -> Void
    ) async -> InboundSyncRunResult
    /// Caller must obtain explicit user confirmation for this single original-token replay.
    func recoverUnknownAcknowledgement(
        scope: ClientMutationScope, position: SyncJournalPosition, confirmedByUser: Bool,
        progress: @MainActor (InboundSyncProgress) -> Void
    ) async -> InboundSyncRunResult
}

@MainActor
protocol SyncCheckpointPreparationProtocol {
    func prepare(scope: ClientMutationScope) async -> SyncCheckpointPreparationResult
}
