import Foundation

/// Production authority delegates to the same serialized connection as queue, inbox and cache.
struct CommittedSQLiteSyncProjectionStorage: CommittedSyncProjectionStorageProtocol {
    let database: MutationQueueSQLiteStore
    let bridge: any RustBridgeProtocol
    func claimAppliedPage(
        scope: ClientMutationScope, position: SyncJournalPosition, credentialId: String
    ) async throws -> AppliedSyncPageProof {
        try await database.claimProjectionPage(
            scope: scope, position: position, credentialId: credentialId, bridge: bridge,
            recovery: false)
    }
    func recoverAppliedPage(
        scope: ClientMutationScope, position: SyncJournalPosition, credentialId: String
    ) async throws -> AppliedSyncPageProof {
        try await database.claimProjectionPage(
            scope: scope, position: position, credentialId: credentialId, bridge: bridge,
            recovery: true)
    }
    func validateAppliedPage(_ proof: AppliedSyncPageProof, credentialId: String) async throws {
        try await database.validateProjectionProof(proof, credentialId: credentialId)
    }
    func authorizeAckDispatch(_ proof: AppliedSyncPageProof, credentialId: String) async throws {
        try await database.authorizeProjectionAckDispatch(proof, credentialId: credentialId)
    }
    func finishAppliedAttempt(_ proof: AppliedSyncPageProof) async {
        await database.finishProjectionAttempt(proof)
    }
    func blockAppliedPage(
        _ proof: AppliedSyncPageProof, reason: SyncFeedFailure, credentialId: String
    ) async throws {
        guard
            [.rebaselineRequired, .checkpointConflict, .checkpointAheadOfProjection].contains(
                reason)
        else { throw SyncFeedFailure.protocolFailure }
        try await database.blockProjectionPage(proof, credentialId: credentialId)
    }
    func confirmAppliedPage(
        _ proof: AppliedSyncPageProof, checkpoint: SyncCheckpoint, credentialId: String
    ) async throws {
        try await database.confirmProjectionPage(
            proof, checkpoint: checkpoint, credentialId: credentialId, bridge: bridge)
    }
}
